use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(test)]
pub use cloud_object_client::GetCloudObjectResponse;
pub use cloud_object_client::InitialLoadResponse;
use futures::channel::oneshot::{self, Receiver};
use futures::stream::AbortHandle;
use itertools::Itertools;
use lazy_static::lazy_static;
use regex::Regex;
use warp_errors::report_error;
use warp_graphql::mcp_gallery_template::MCPGalleryTemplate;
use warp_graphql::object_permissions::AccessLevel;
use warp_graphql::scalars::time::ServerTimestamp;
use warp_util::sync::Condition;
use warpui::r#async::{FutureId, Timer};
use warpui::{
    AppContext, Entity, ModelContext, ModelHandle, RequestState, RetryOption, SingletonEntity,
    duration_with_jitter,
};

use cloud_object_client::ObjectUpdateMessage;
// LOCAL FORK: a `#[cfg(not(target_family = "wasm"))]` attribute sat here and was not ours to keep.
// On `main` it belongs to an import the excision deleted; removing the item without
// its attribute rebound it to the line below, which `main` leaves ungated. That hid
// these symbols from every build where the condition is false.
use crate::auth::AuthStateProvider;
use crate::cloud_object::CloudObjectTypeAndId;
use crate::cloud_object::folders::{CloudFolderModel, FolderId};
use crate::cloud_object::model::actions::{
    ObjectAction, ObjectActionHistory, ObjectActionType, ObjectActions,
};
use crate::cloud_object::model::generic_string_model::{
    GenericStringModel, GenericStringObjectId, Serializer, StringModel,
};
use crate::cloud_object::model::persistence::{CloudModel, CloudModelEvent, UpdateSource};
use crate::cloud_object::model::view::{CloudViewModel, Editor, EditorState};
use crate::cloud_object::{
    CloudLinkSharing, CloudModelType, CloudObject, CloudObjectEventEntrypoint, CloudObjectLocation,
    CloudObjectSyncStatus, CreateCloudObjectResult, CreateObjectRequest, GenericCloudObject,
    GenericServerObject, GenericStringObjectFormat, JsonObjectType, NumInFlightRequests,
    ObjectDeleteResult, ObjectIdType, ObjectMetadataUpdateResult, ObjectPermissionsUpdateData,
    ObjectType, Owner, Revision, RevisionAndLastEditor, ServerCloudObject, ServerEnvVarCollection,
    ServerMetadata, ServerPermissions, ServerPreference, ServerWorkflowEnum, Space,
    UpdateCloudObjectResult,
};
use crate::env_vars::{CloudEnvVarCollectionModel, EnvVarCollection};
use crate::network::{NetworkStatus, NetworkStatusEvent, NetworkStatusKind};
use crate::notebooks::{CloudNotebookModel, NotebookId};
use crate::persistence::ModelEvent;
use crate::server::ids::{
    ClientId, HashableId, HashedSqliteId, ObjectUid, ServerId, SyncId, ToServerId,
    parse_sqlite_id_to_uid,
};
use crate::server::retry_strategies::{
    OUT_OF_BAND_REQUEST_RETRY_STRATEGY, PERIODIC_POLL, PERIODIC_POLL_RETRY_STRATEGY,
};
use crate::server::server_api::object::{GuestIdentifier, ObjectClient};
use crate::settings::cloud_preferences::Preference;
use crate::sharing::SharingAccessLevel;
use crate::workflows::workflow::Workflow;
use crate::workflows::workflow_enum::{CloudWorkflowEnum, CloudWorkflowEnumModel, WorkflowEnum};
use crate::workflows::{CloudWorkflowModel, WorkflowId};
use crate::workspaces::team_tester::{TeamTesterStatus, TeamTesterStatusEvent};
use crate::workspaces::update_manager::TeamUpdateManager;
use crate::workspaces::user_profiles::{UserProfileWithUID, UserProfiles};
use crate::workspaces::user_workspaces::UserWorkspaces;

lazy_static! {
    /// For online-only operations, we want to quickly determine if the operation can succeed,
    /// so that if it can't, we can put the user back into the known good state.
    /// So we try 3 times to prevent any transient failures.
    static ref ONLINE_ONLY_OPERATION_RETRY_STRATEGY: RetryOption =
        RetryOption::exponential(Duration::from_millis(500) /* interval */, 2. /* exponential factor */, 3 /* max retry count */);

    static ref DUPLICATE_OBJECT_NAME_REGEX: Regex = Regex::new(r" \((\d+)\)$").expect("regex should not fail to compile");

}

#[derive(Debug, PartialEq)]
pub enum OperationSuccessType {
    Success,
    Failure,
    Rejection,
    Denied(String),
    FeatureNotAvailable,
}

#[derive(Debug, PartialEq)]
pub enum ObjectOperation {
    Create { initiated_by: InitiatedBy },
    Update,
    Trash,
    TakeEditAccess,
    Untrash,
    Delete { initiated_by: InitiatedBy },
    UpdatePermissions,
}

#[derive(Debug)]
pub struct ObjectOperationResult {
    pub success_type: OperationSuccessType,
    pub operation: ObjectOperation,
    pub client_id: Option<ClientId>,
    /// LOCAL FORK: was `server_id: Option<ServerId>`.
    ///
    /// Every operation that produced this result used to be online-only, so the object
    /// always had a server id by the time one was emitted, and consumers matched on it to
    /// find out whether the result was about the object they were showing. Trashing,
    /// untrashing, deleting and taking the notebook edit baton are local operations now
    /// and run on objects that have only ever had a client id, so a server id is the one
    /// identity they cannot supply. Filling it in with `ServerId::from_string_lossy`
    /// would have been worse than useless: a client uid is a 36-character uuid, a server
    /// id is 22 characters, and that constructor panics on the mismatch under
    /// `debug_assertions`.
    pub object_id: Option<SyncId>,
    pub num_objects: Option<i32>, // counts number of objects (including descendants) deleted for permadeletion
}

#[derive(Debug)]
pub enum UpdateManagerEvent {
    ObjectOperationComplete { result: ObjectOperationResult },
}

/// An enum that defines whether the action was initiated by the user or the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitiatedBy {
    User,
    System,
}

#[derive(Debug)]
pub struct GenericStringObjectInput<T, S>
where
    T: StringModel<
            CloudObjectType = GenericCloudObject<GenericStringObjectId, GenericStringModel<T, S>>,
        > + 'static,
    S: Serializer<T> + 'static,
{
    pub id: ClientId,
    pub model: GenericStringModel<T, S>,
    pub initial_folder_id: Option<SyncId>,
    pub entrypoint: CloudObjectEventEntrypoint,
}

/// The UpdateManager is responsible for delegating work when there is an update to an
/// object (e.g. via a user interaction). Specifically, it will
/// - write to SQLite
/// - interact with the CloudModel to update the in-memory state used by the object views
///
/// LOCAL FORK: it no longer talks to a server, and so no longer holds an `ObjectClient`.
///
/// It used to be the junction between three parties: the views that mutate objects, the
/// sync queue that carried those mutations to the backend, and the real-time channel that
/// carried other clients' mutations back. Both server-facing sides are gone.
///
/// Outbound, every write is now purely local: the in-memory model and sqlite are updated
/// and that is the end of the operation. The methods that could only run online -- moving
/// between spaces, sharing, the notebook edit baton, trashing -- have been reduced to
/// their local halves rather than deleted, because each one backs a menu entry a user can
/// still reach.
///
/// Inbound, the whole response and push path went: the `SyncQueueEvent` handler that
/// turned a client id into a server id, the conflict machinery that ran when the server
/// rejected an update as stale, the permissions and metadata handlers, and
/// `received_message_from_server`. That last one had already lost its production caller
/// when the real-time channel was removed; only tests still reached it.
pub struct UpdateManager {
    model_event_sender: Option<SyncSender<ModelEvent>>,
    spawned_futures: Vec<FutureId>,
}

impl UpdateManager {
    pub fn new(
        model_event_sender: Option<SyncSender<ModelEvent>>,
        _ctx: &mut ModelContext<Self>,
    ) -> Self {
        Self {
            model_event_sender,
            spawned_futures: Default::default(),
        }
    }

    #[cfg(test)]
    pub fn mock(ctx: &mut ModelContext<Self>) -> Self {
        Self::new(None, ctx)
    }

    #[cfg(any(test, feature = "integration_tests"))]
    pub fn spawned_futures(&self) -> &[FutureId] {
        &self.spawned_futures
    }

    fn save_to_db(&self, events: impl IntoIterator<Item = ModelEvent>) {
        let model_event_sender = self.model_event_sender.clone();
        if let Some(model_event_sender) = &model_event_sender {
            for event in events {
                if let Err(e) = model_event_sender.send(event) {
                    report_error!(anyhow::Error::new(e).context("Error saving to database"));
                }
            }
        }
    }

    fn save_in_memory_object_to_sqlite(&mut self, cloud_model: &CloudModel, uid: &ObjectUid) {
        if let Some(cloud_object) = cloud_model.get_by_uid(uid) {
            self.save_to_db([cloud_object.upsert_event()]);
        }
    }

    fn save_in_memory_object_metadata_to_sqlite(
        &mut self,
        cloud_model: &CloudModel,
        uid: &ObjectUid,
        hashed_sqlite_id: &str,
    ) {
        if let Some(cloud_object) = cloud_model.get_by_uid(uid) {
            let metadata = cloud_object.metadata().clone();
            let event = ModelEvent::UpdateObjectMetadata {
                id: hashed_sqlite_id.to_string(),
                metadata,
            };
            self.save_to_db([event]);
        }
    }

    pub fn update_workflow(
        &mut self,
        workflow: Workflow,
        workflow_id: SyncId,
        revision_ts: Option<Revision>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(
            CloudWorkflowModel::new(workflow),
            workflow_id,
            revision_ts,
            ctx,
        );
    }

    pub fn update_workflow_enum(
        &mut self,
        workflow_enum: WorkflowEnum,
        workflow_enum_id: SyncId,
        revision_ts: Option<Revision>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(
            CloudWorkflowEnumModel::new(workflow_enum),
            workflow_enum_id,
            revision_ts,
            ctx,
        );
    }

    pub fn update_env_var_collection(
        &mut self,
        env_var_collection: EnvVarCollection,
        env_var_collection_id: SyncId,
        revision_ts: Option<Revision>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(
            CloudEnvVarCollectionModel::new(env_var_collection),
            env_var_collection_id,
            revision_ts,
            ctx,
        );
    }

    pub fn update_notebook_data(
        &mut self,
        data: Arc<String>,
        notebook_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        let cloud_model = CloudModel::as_ref(ctx);
        let revision = cloud_model.current_revision(&notebook_id).cloned();
        if let Some(notebook) = cloud_model.get_notebook(&notebook_id) {
            let new_notebook = CloudNotebookModel {
                title: notebook.model().title.to_owned(),
                data: data.to_string(),
                ai_document_id: notebook.model().ai_document_id,
                conversation_id: notebook.model().conversation_id.clone(),
            };
            self.update_object(new_notebook, notebook_id, revision, ctx);
        } else {
            log::warn!("Expected notebook to be in model with id {notebook_id:?}");
        }
    }

    pub fn update_notebook_title(
        &mut self,
        title: Arc<String>,
        notebook_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        let cloud_model = CloudModel::as_ref(ctx);
        let revision = cloud_model.current_revision(&notebook_id).cloned();
        if let Some(notebook) = cloud_model.get_notebook(&notebook_id) {
            let new_notebook = CloudNotebookModel {
                title: title.to_string(),
                data: notebook.model().data.to_owned(),
                ai_document_id: notebook.model().ai_document_id,
                conversation_id: notebook.model().conversation_id.clone(),
            };
            self.update_object(new_notebook, notebook_id, revision, ctx);
        } else {
            log::warn!("Expected notebook to be in model with id {notebook_id:?}");
        }
    }

    // This method moves an object from its current location to a new location.
    // Since moving is an online-only operation, this operation does NOT go through the sync queue.
    pub fn move_object_to_location(
        &mut self,
        object_id: CloudObjectTypeAndId,
        new_location: CloudObjectLocation,
        ctx: &mut ModelContext<Self>,
    ) {
        // If we are moving into the trash, we really mean to trash the object
        if let CloudObjectLocation::Trash = new_location {
            return self.trash_object(object_id, ctx);
        }

        // LOCAL FORK: moving between spaces and folders was an online-only operation. It
        // required a server ID, which a locally created object never gets, and then asked
        // the server to perform the move. The object type and the metadata and permissions
        // timestamps went with the outbound requests, which were their only readers. What
        // remains is the optimistic in-memory update and the revert that follows it.
        let uid = object_id.uid();

        let Some((object_current_owner, object_current_folder, has_pending_online_only_change)) =
            CloudModel::handle(ctx).read(ctx, |model, _| {
                let object = model.get_by_uid(&uid)?;
                Some((
                    object.permissions().owner,
                    object.metadata().folder_id,
                    object.metadata().has_pending_online_only_change(),
                ))
            })
        else {
            return;
        };

        // We disallow stacked online-only changes so early return
        // if there's already one pending for this object.
        if has_pending_online_only_change {
            return;
        }

        // Apply a pending, optimistic update and then try to sync the move with the server.
        // We only update the in-memory data but don't persist anything in sqlite until the server confirms the move.
        // Todo: this logic shouldn't need to match based on Space versus Folder. Once we have moving across spaces in MoveObject,
        // we should simplify this to a unified call to move_object that sends the new space AND the new folder.
        let mut not_supported = false;
        match new_location {
            CloudObjectLocation::Space(destination_space) => {
                match UserWorkspaces::as_ref(ctx).space_to_owner(destination_space, ctx) {
                    Some(destination_owner) => {
                        if destination_owner == object_current_owner {
                            // If the space is staying the same, then the move must be to move to the root of the space.
                            CloudModel::handle(ctx).update(ctx, |model, ctx| {
                                model.update_object_location(&uid, None, None, ctx);
                            });
                        } else {
                            CloudModel::handle(ctx).update(ctx, |model, ctx| {
                                model.update_object_location(
                                    &uid,
                                    Some(destination_owner),
                                    None,
                                    ctx,
                                );
                            });
                        }
                    }
                    None => {
                        // We couldn't map the space to a valid owner (most likely, it's the
                        // "shared" space).
                        not_supported = true;
                    }
                }
            }
            CloudObjectLocation::Folder(SyncId::ServerId(destination_folder_id)) => {
                // If we're moving across folders, then the space must be staying the same.
                CloudModel::handle(ctx).update(ctx, |model, ctx| {
                    model.update_object_location(
                        &uid,
                        None,
                        Some(SyncId::ServerId(destination_folder_id)),
                        ctx,
                    );
                });
            }
            _ => {
                not_supported = true;
            }
        }

        // In all other cases, just immediately revert the optimistic update since
        // we won't be trying to move the object and we don't want the object to appear
        // as pending.
        if not_supported {
            CloudModel::handle(ctx).update(ctx, |model, ctx| {
                model.update_object_location(
                    &uid,
                    Some(object_current_owner),
                    object_current_folder,
                    ctx,
                );
            });
        }

        ctx.notify();
    }

    pub fn duplicate_object(
        &mut self,
        cloud_object_type_and_id: &CloudObjectTypeAndId,
        ctx: &mut ModelContext<Self>,
    ) {
        match cloud_object_type_and_id {
            CloudObjectTypeAndId::Notebook(notebook_id) => {
                self.duplicate_object_internal::<NotebookId, CloudNotebookModel>(notebook_id, ctx);
            }
            CloudObjectTypeAndId::Workflow(workflow_id) => {
                self.duplicate_object_internal::<WorkflowId, CloudWorkflowModel>(workflow_id, ctx);
            }
            CloudObjectTypeAndId::GenericStringObject { object_type, id } => {
                if let GenericStringObjectFormat::Json(JsonObjectType::EnvVarCollection) =
                    object_type
                {
                    self.duplicate_object_internal::<GenericStringObjectId, CloudEnvVarCollectionModel>(
                        id, ctx,
                    );
                } else {
                    report_error!("Tried to duplicate an unsupported type: json object");
                    debug_assert!(false, "Tried to duplicate an unsupported type: json object");
                }
            }
            CloudObjectTypeAndId::Folder(_) => {
                // Duplicating folders not currently supported.
                report_error!("Tried to duplicate an unsupported type: folder");
                debug_assert!(false, "Tried to duplicate an unsupported type: folder");
            }
        }
    }

    fn duplicate_object_internal<K, M>(&mut self, id: &SyncId, ctx: &mut ModelContext<Self>)
    where
        K: HashableId
            + ToServerId
            + std::fmt::Debug
            + Into<String>
            + Clone
            + Copy
            + Send
            + Sync
            + 'static,
        M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
    {
        let (duplicate_model, client_id, owner, initial_folder_id, entrypoint) = {
            let cloud_model = CloudModel::as_ref(ctx);
            let object: GenericCloudObject<K, M> = cloud_model
                .get_object_of_type(id)
                .expect("object should exist in order to be duplicated")
                .clone();
            let client_id = ClientId::new();
            let owner = object.permissions.owner;
            let initial_folder_id = object.metadata.folder_id;
            let entrypoint = CloudObjectEventEntrypoint::Unknown;
            let mut duplicate_model = object.model().clone();
            let duplicate_name =
                self.get_next_duplicate_object_name(&object as &dyn CloudObject, cloud_model, ctx);
            duplicate_model.set_display_name(&duplicate_name);
            (
                duplicate_model,
                client_id,
                owner,
                initial_folder_id,
                entrypoint,
            )
        };
        self.create_object(
            duplicate_model,
            owner,
            client_id,
            entrypoint,
            true,
            initial_folder_id,
            // When adding the initiated_by parameter to this function call, InitiatedBy::User was set as a default value.
            // This can be changed to InitiatedBy::System if this action was automatically kicked off by the system and we do not want a user facing toast.
            InitiatedBy::User,
            ctx,
        );
    }

    pub fn delete_ai_execution_profile(
        &mut self,
        ai_execution_profile_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.delete_object_by_user(
            CloudObjectTypeAndId::GenericStringObject {
                object_type: GenericStringObjectFormat::Json(JsonObjectType::AIExecutionProfile),
                id: ai_execution_profile_id,
            },
            ctx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_notebook(
        &mut self,
        client_id: ClientId,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
        model: CloudNotebookModel,
        entrypoint: CloudObjectEventEntrypoint,
        force_expand: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        // LOCAL FORK: an anonymous-user object limit guard stood here. It only ever
        // fired for feature-gated anonymous cloud accounts, which this build cannot
        // create, so it was already dead: `is_anonymous_user_feature_gated()` returns
        // `None` with no user and the predicate folded to `false`. Personal objects are
        // now unlimited, matching what a logged-in user saw upstream.

        self.create_object(
            model,
            owner,
            client_id,
            entrypoint,
            force_expand,
            initial_folder_id,
            // When adding the initiated_by parameter to this function call, InitiatedBy::User was set as a default value.
            // This can be changed to InitiatedBy::System if this action was automatically kicked off by the system and we do not want a user facing toast.
            InitiatedBy::User,
            ctx,
        );
    }

    fn get_next_duplicate_object_name(
        &self,
        original_cloud_object: &dyn CloudObject,
        cloud_model: &CloudModel,
        app: &AppContext,
    ) -> String {
        let original_name = original_cloud_object.display_name();

        // Iterate through items in the same folder as the original object that are of the
        // same type, and populate a hashset with those names.
        let same_type_and_folder_names = cloud_model
            .active_cloud_objects_in_location_without_descendents(
                original_cloud_object.location(cloud_model, app),
                app,
            )
            .filter(|&object| object.object_type() == original_cloud_object.object_type())
            .map(|object| object.display_name())
            .collect::<HashSet<String>>();

        // Start with "{original_object_name} ({original_object_name's count + 1})".
        // Keep incrementing by one if there already exists an object of the same type in
        // the same folder (using the hashset generated above).
        let mut duplicate_name = get_duplicate_object_name(&original_name);
        while same_type_and_folder_names.contains(&duplicate_name) {
            duplicate_name = get_duplicate_object_name(&duplicate_name);
        }
        duplicate_name
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_workflow(
        &mut self,
        workflow: Workflow,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
        client_id: ClientId,
        entrypoint: CloudObjectEventEntrypoint,
        force_expand: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        // LOCAL FORK: an anonymous-user object limit guard stood here. It only ever
        // fired for feature-gated anonymous cloud accounts, which this build cannot
        // create, so it was already dead: `is_anonymous_user_feature_gated()` returns
        // `None` with no user and the predicate folded to `false`. Personal objects are
        // now unlimited, matching what a logged-in user saw upstream.

        self.create_object(
            CloudWorkflowModel::new(workflow),
            owner,
            client_id,
            entrypoint,
            force_expand,
            initial_folder_id,
            // When adding the initiated_by parameter to this function call, InitiatedBy::User was set as a default value.
            // This can be changed to InitiatedBy::System if this action was automatically kicked off by the system and we do not want a user facing toast.
            InitiatedBy::User,
            ctx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_workflow_enum(
        &mut self,
        workflow_enum: WorkflowEnum,
        owner: Owner,
        client_id: ClientId,
        entrypoint: CloudObjectEventEntrypoint,
        force_expand: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        self.create_object(
            CloudWorkflowEnumModel::new(workflow_enum),
            owner,
            client_id,
            entrypoint,
            force_expand,
            None,
            // When adding the initiated_by parameter to this function call, InitiatedBy::User was set as a default value.
            // This can be changed to InitiatedBy::System if this action was automatically kicked off by the system and we do not want a user facing toast.
            InitiatedBy::User,
            ctx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_env_var_collection(
        &mut self,
        client_id: ClientId,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
        model: CloudEnvVarCollectionModel,
        entrypoint: CloudObjectEventEntrypoint,
        force_expand: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        // LOCAL FORK: an anonymous-user object limit guard stood here. It only ever
        // fired for feature-gated anonymous cloud accounts, which this build cannot
        // create, so it was already dead: `is_anonymous_user_feature_gated()` returns
        // `None` with no user and the predicate folded to `false`. Personal objects are
        // now unlimited, matching what a logged-in user saw upstream.

        self.create_object(
            model,
            owner,
            client_id,
            entrypoint,
            force_expand,
            initial_folder_id,
            // When adding the initiated_by parameter to this function call, InitiatedBy::User was set as a default value.
            // This can be changed to InitiatedBy::System if this action was automatically kicked off by the system and we do not want a user facing toast.
            InitiatedBy::User,
            ctx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_folder(
        &mut self,
        name: String,
        owner: Owner,
        client_id: ClientId,
        initial_folder_id: Option<SyncId>,
        force_expand: bool,
        initiated_by: InitiatedBy,
        ctx: &mut ModelContext<Self>,
    ) {
        self.create_object(
            // TODO(INT-789): support creating folders as warp packs
            CloudFolderModel::new(&name, false),
            owner,
            client_id,
            Default::default(),
            force_expand,
            initial_folder_id,
            initiated_by,
            ctx,
        );
    }

    /// Bulk creates a list of generic string objects, all in a single
    /// sqllite write and server api call.  More efficient than calling
    /// create_object for each object.
    ///
    /// Note that if the bulk creation request fails, the client will end up retrying
    /// object creation one write and request at a time.
    pub fn bulk_create_generic_string_objects<S, T>(
        &mut self,
        owner: Owner,
        inputs: Vec<GenericStringObjectInput<T, S>>,
        ctx: &mut ModelContext<Self>,
    ) where
        T: StringModel<
                CloudObjectType = GenericCloudObject<
                    GenericStringObjectId,
                    GenericStringModel<T, S>,
                >,
            > + 'static,
        S: Serializer<T> + 'static,
    {
        let mut objects = Vec::new();
        for input in inputs {
            let object_id = SyncId::ClientId(input.id);

            // Update in-memory model.
            CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
                let object =
                GenericCloudObject::<GenericStringObjectId, GenericStringModel<T, S>>::new_local(
                    input.model,
                    owner,
                    input.initial_folder_id,
                    input.id,
                );
                cloud_model.create_object(object_id, object, ctx);
            });

            let cloud_model = CloudModel::as_ref(ctx);
            if let Some(object) = cloud_model
                .get_object_of_type::<GenericStringObjectId, GenericStringModel<T, S>>(&object_id)
            {
                objects.push(object.clone());
            }
        }

        // Update sqlite with a single bulk request
        self.save_to_db(vec![GenericStringModel::<T, S>::bulk_upsert_event(
            objects
                .iter()
                .map(|object| object.upsert_params(object.object_type()))
                .collect(),
        )]);
    }

    /// Generic function for creating a new cloud object with a given model.
    #[allow(clippy::too_many_arguments)]
    pub fn create_object<K, M>(
        &mut self,
        model: M,
        owner: Owner,
        client_id: ClientId,
        entrypoint: CloudObjectEventEntrypoint,
        force_expand: bool,
        initial_folder_id: Option<SyncId>,
        initiated_by: InitiatedBy,
        ctx: &mut ModelContext<Self>,
    ) where
        K: HashableId
            + ToServerId
            + std::fmt::Debug
            + Into<String>
            + Clone
            + Copy
            + Send
            + Sync
            + 'static,
        M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
    {
        let object_id = SyncId::ClientId(client_id);
        let auth_state = AuthStateProvider::as_ref(ctx).get();
        let initial_editor = auth_state.user_id();

        // Update in-memory model.
        CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
            let mut object = GenericCloudObject::<K, M>::new_local(
                model.clone(),
                owner,
                initial_folder_id,
                client_id,
            );
            object.metadata.current_editor_uid = initial_editor.map(|uid| uid.as_string());
            cloud_model.create_object(object_id, object, ctx);

            if force_expand {
                cloud_model.force_expand_object_and_ancestors(object_id, ctx);
            }
        });

        // Update sqlite.
        let cloud_model = CloudModel::as_ref(ctx);
        if let Some(object) = cloud_model.get_object_of_type::<K, M>(&object_id) {
            self.save_to_db([object.upsert_event()]);
        }
    }

    // LOCAL FORK: `create_object_online` and `update_object_online` both went with cloud
    // sync. They pushed a create or an update straight to the server, outside the sync
    // queue, so a caller that managed its own retries (the CLI) would not race the queue
    // into creating duplicates. Unlike every other write path they touched the cloud model
    // and sqlite only after the server answered, which with no server meant never.

    /// Generic function for updating a cloud object with a new model.
    pub fn update_object<K, M>(
        &mut self,
        model: M,
        object_id: SyncId,
        revision_ts: Option<Revision>,
        ctx: &mut ModelContext<Self>,
    ) where
        K: HashableId
            + ToServerId
            + std::fmt::Debug
            + Into<String>
            + Clone
            + Copy
            + Send
            + Sync
            + 'static,
        M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
    {
        // Update in-memory model.
        //
        // LOCAL FORK: the object is no longer marked in-flight here. That counter tracked
        // outstanding server requests, and it was decremented by the response handler. With
        // the request gone the counter would only ever go up, leaving every object the user
        // edited permanently `InFlight` -- which is what `has_pending_content_changes`
        // reports as unsaved, and what `num_unsaved_objects_to_warn_about_before_quitting`
        // counts. Every quit after any edit would have warned about unsaved work that was
        // in fact already on disk. Writing to sqlite below *is* the save now, so the object
        // stays at `NoLocalChanges` and shows the "Saved locally" sync tooltip.
        CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
            cloud_model.update_object_from_edit(model.clone(), object_id, ctx);
            ctx.notify();
        });

        // Update sqlite.
        let cloud_model = CloudModel::as_ref(ctx);
        if let Some(object) = cloud_model.get_object_of_type::<K, M>(&object_id) {
            self.save_to_db([object.upsert_event()]);
        };
    }

    // Takes a generic SyncId and records the action.
    pub fn record_object_action(
        &mut self,
        id_and_type: CloudObjectTypeAndId,
        action_type: ObjectActionType,
        data: Option<String>,
        ctx: &mut ModelContext<Self>,
    ) {
        // Take the action timestamp from the client.
        let action_timestamp = Utc::now();

        // Update in-memory model.
        let object_action = ObjectActions::handle(ctx).update(ctx, |object_actions_model, ctx| {
            object_actions_model.insert_action(
                id_and_type.uid(),
                id_and_type.sqlite_uid_hash(),
                action_type.clone(),
                data.clone(),
                action_timestamp,
                ctx,
            )
        });

        // Update sqlite.
        //
        // LOCAL FORK: the action is still recorded as `pending`, which used to mean "not
        // yet reported to the server". Nothing reads that flag any more -- the queue item
        // it fed, the response that cleared it and the history merge that preserved it are
        // all gone -- but it is part of the persisted shape of an action, so it is left
        // alone rather than changed under existing sqlite rows.
        self.save_to_db([ModelEvent::InsertObjectAction { object_action }]);
    }

    /// Sets the notebooks current editor in memory. SQLite is not updated until we receive
    /// server confirmation.
    fn set_notebook_current_editor(
        &self,
        notebook_id: &SyncId,
        editor_uid: Option<String>,
        ctx: &mut ModelContext<Self>,
    ) {
        CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
            if let Some(notebook) = cloud_model.get_notebook_mut(notebook_id) {
                notebook.metadata.set_current_editor(editor_uid);
                ctx.notify();
            }
        });
    }

    /// LOCAL FORK: takes the notebook's edit baton locally.
    ///
    /// The baton exists so two people editing the same shared notebook do not overwrite
    /// each other: the server held the current editor, and this asked to become it. That
    /// required a server ID, which a notebook created in this build never has, so the
    /// method returned before doing anything.
    ///
    /// For the common call, `optimistically_grant_access = true`, that was invisible: the
    /// caller switches to edit mode itself and only used the request to tell the server.
    /// The other call is the one that mattered. `grab_edit_access(false, ..)` comes from
    /// the "someone else is editing" modal, and the caller does *not* switch to edit mode
    /// there; it waits for `ObjectOperation::TakeEditAccess` to come back successful. With
    /// no server the event never arrived, so pressing Take Access did nothing. Nothing
    /// creates that state locally, but a notebook synced before this fork can still carry
    /// another user's uid in its persisted metadata, and then the modal is reachable.
    ///
    /// One local user cannot contend with anyone, so taking the baton always succeeds.
    pub fn grab_notebook_edit_access(
        &mut self,
        notebook_id: SyncId,
        optimistically_grant_access: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        let auth_state = AuthStateProvider::as_ref(ctx).get();
        let user_uid = auth_state.user_id().unwrap_or_default();
        self.set_notebook_current_editor(&notebook_id, Some(user_uid.as_string()), ctx);

        if !optimistically_grant_access {
            ctx.emit(UpdateManagerEvent::ObjectOperationComplete {
                result: ObjectOperationResult {
                    success_type: OperationSuccessType::Success,
                    operation: ObjectOperation::TakeEditAccess,
                    client_id: None,
                    object_id: Some(notebook_id),
                    num_objects: None,
                },
            });
            ctx.notify();
        }
    }

    /// LOCAL FORK: releases the notebook's edit baton locally.
    ///
    /// The request that told the server went with [`Self::grab_notebook_edit_access`]. The
    /// local clear stays: the notebook view calls this when it leaves edit mode, and the
    /// details bar reads the current editor to decide what to show.
    pub fn give_up_notebook_edit_access(
        &mut self,
        notebook_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        let current_editor = CloudViewModel::as_ref(ctx)
            .object_current_editor(&notebook_id.uid(), ctx)
            .unwrap_or(Editor::no_editor());

        // Only give up access if the current user has edit access.
        if matches!(current_editor.state, EditorState::CurrentUser) {
            self.set_notebook_current_editor(&notebook_id, None, ctx);
        }
    }
    /// LOCAL FORK: marks an object trashed locally.
    ///
    /// Was `mark_object_trashed_and_return_timestamps`. It returned the metadata and
    /// trashed timestamps so the caller could tell, when a request came back a failure,
    /// whether the metadata had moved on in the meantime and the optimistic write was
    /// therefore no longer safe to revert. There is no request and no revert now, so
    /// nobody reads them.
    fn mark_object_trashed(&self, uid: &ObjectUid, ctx: &mut ModelContext<Self>) {
        let timestamp = ServerTimestamp::new(Utc::now());
        CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
            if let Some(object) = cloud_model.get_mut_by_uid(uid) {
                object.metadata_mut().trashed_ts = Some(timestamp);
                ctx.emit(CloudModelEvent::ObjectTrashed {
                    type_and_id: object.cloud_object_type_and_id(),
                    source: UpdateSource::Local,
                });
                ctx.notify();
            }
        });
    }

    /// LOCAL FORK: trashing is a local operation.
    ///
    /// This used to require a server ID and then ask the server to set the trashed
    /// timestamp, treating the local write as optimistic until the response landed. An
    /// object created in this build never gets a server ID, so `id.server_id()` returned
    /// `None` and the method returned before touching anything. The Trash entry stayed
    /// enabled in the workflow, notebook, env-var-collection and workflow-argument menus
    /// and silently did nothing, which left no way to remove a workflow you had made.
    ///
    /// The timestamp the server used to mint is now written here and persisted, which is
    /// what the rest of the app reads to decide an object sits in the trash. There is no
    /// request to fail, so nothing reverts and no pending-metadata flag is held open.
    pub fn trash_object(&mut self, id: CloudObjectTypeAndId, ctx: &mut ModelContext<Self>) {
        let hashed_id = id.uid();

        // If there's a pending online-only operation for this object, don't trash it.
        let Some(has_pending_online_only_operation) =
            CloudModel::handle(ctx).read(ctx, |model, _| {
                model
                    .get_by_uid(&hashed_id)
                    .map(|object| object.metadata().has_pending_online_only_change())
            })
        else {
            return;
        };

        if has_pending_online_only_operation {
            return;
        }

        self.mark_object_trashed(&hashed_id, ctx);

        let cloud_model = CloudModel::as_ref(ctx);
        self.save_in_memory_object_to_sqlite(cloud_model, &hashed_id);

        ctx.emit(UpdateManagerEvent::ObjectOperationComplete {
            result: ObjectOperationResult {
                success_type: OperationSuccessType::Success,
                operation: ObjectOperation::Trash,
                client_id: None,
                object_id: Some(id.sync_id()),
                num_objects: None,
            },
        });
        ctx.notify();
    }

    /// LOCAL FORK: untrashing is a local operation, for the same reason as
    /// [`Self::trash_object`].
    ///
    /// Upstream deliberately did not clear `trashed_ts` optimistically. It left the
    /// object looking trashed and waited for the server's metadata, so the restore would
    /// reflect canonical timestamps rather than a guess. With no later authority to defer
    /// to, the field is cleared here and persisted.
    pub fn untrash_object(&mut self, id: CloudObjectTypeAndId, ctx: &mut ModelContext<Self>) {
        let hashed_id = id.uid();

        // If there's a pending online-only operation for this object, don't untrash it.
        let Some(has_pending_online_only_operation) =
            CloudModel::handle(ctx).read(ctx, |model, _| {
                model
                    .get_by_uid(&hashed_id)
                    .map(|object| object.metadata().has_pending_online_only_change())
            })
        else {
            return;
        };

        if has_pending_online_only_operation {
            return;
        }

        CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
            if let Some(object) = cloud_model.get_mut_by_uid(&hashed_id) {
                object.metadata_mut().trashed_ts = None;
                ctx.emit(CloudModelEvent::ObjectUntrashed {
                    type_and_id: object.cloud_object_type_and_id(),
                    source: UpdateSource::Local,
                });
                ctx.notify();
            }
        });

        let cloud_model = CloudModel::as_ref(ctx);
        self.save_in_memory_object_to_sqlite(cloud_model, &hashed_id);

        ctx.emit(UpdateManagerEvent::ObjectOperationComplete {
            result: ObjectOperationResult {
                success_type: OperationSuccessType::Success,
                operation: ObjectOperation::Untrash,
                client_id: None,
                object_id: Some(id.sync_id()),
                num_objects: None,
            },
        });
        ctx.notify();
    }

    /// LOCAL FORK: deletes an object and everything nested under it, locally.
    ///
    /// The server round trip is gone. It asked the backend to delete, and the backend
    /// answered with the full set of ids it had removed, because deleting a folder
    /// deletes its contents and only the server knew the closure. `delete_objects_by_id`
    /// already walks that closure in the in-memory model and reports what it removed, so
    /// the local half was always capable of computing the same set; it simply deferred.
    ///
    /// `delete_object_with_initiated_by` went with the request. The `initiated_by` flag
    /// existed to decide whether a *failure* toast was worth showing the user or was
    /// noise from a background sync. Nothing can fail here.
    pub fn delete_object_by_user(
        &mut self,
        id: CloudObjectTypeAndId,
        ctx: &mut ModelContext<Self>,
    ) {
        let uid = id.uid();

        // If there's a pending online-only operation for this object, don't delete it.
        let Some(has_pending_online_only_operation) =
            CloudModel::handle(ctx).read(ctx, |model, _| {
                model
                    .get_by_uid(&uid)
                    .map(|object| object.metadata().has_pending_online_only_change())
            })
        else {
            return;
        };

        if has_pending_online_only_operation {
            return;
        }

        let num_deleted_objects = self.on_object_delete_success(vec![id.sync_id()], ctx);

        ctx.emit(UpdateManagerEvent::ObjectOperationComplete {
            result: ObjectOperationResult {
                success_type: OperationSuccessType::Success,
                operation: ObjectOperation::Delete {
                    initiated_by: InitiatedBy::User,
                },
                client_id: None,
                object_id: Some(id.sync_id()),
                num_objects: Some(num_deleted_objects),
            },
        });
        ctx.notify();
    }

    pub fn on_object_delete_success(
        &mut self,
        deleted_ids: Vec<SyncId>,
        ctx: &mut ModelContext<'_, UpdateManager>,
    ) -> i32 {
        let cloud_model_handle = CloudModel::handle(ctx);
        let all_object_uids: Vec<ObjectUid> = deleted_ids.iter().map(|&id| id.uid()).collect();

        // This variable counts the number of objects deleted client-side in each Empty Trash action,
        // because the server returns everything in the db, including objects that have already been marked for deletion
        let mut num_deleted_objects = 0;
        let mut sync_ids_and_types: Vec<(SyncId, ObjectIdType)> = Vec::new();
        cloud_model_handle.update(ctx, |cloud_model, ctx| {
            (sync_ids_and_types, num_deleted_objects) =
                cloud_model.delete_objects_by_id(all_object_uids.clone(), ctx);
        });

        // Deleted the actions associated with these objects too.
        ObjectActions::handle(ctx).update(ctx, |object_actions, ctx| {
            for uid in all_object_uids.clone() {
                object_actions.delete_actions_for_object(&uid, ctx);
            }
        });

        // Return early if empty
        if num_deleted_objects == 0 {
            return num_deleted_objects;
        }

        // Delete objects from sqlite. This will also delete their actions.
        self.save_to_db([ModelEvent::DeleteObjects {
            ids: sync_ids_and_types,
        }]);

        num_deleted_objects
    }

    pub fn rename_folder(
        &mut self,
        folder_id: SyncId,
        new_name: String,
        ctx: &mut ModelContext<Self>,
    ) {
        let cloud_model = CloudModel::as_ref(ctx);
        let revision = cloud_model.current_revision(&folder_id).cloned();
        if let Some(folder) = cloud_model.get_folder(&folder_id) {
            let new_folder = CloudFolderModel {
                name: new_name,
                is_open: folder.model().is_open,
                is_warp_pack: folder.model().is_warp_pack,
            };
            self.update_object(new_folder, folder_id, revision, ctx);
        } else {
            log::warn!("Attempted to rename folder that doesn't exist with id: {folder_id:?}");
        }
    }
}

/// Return the newly duplicated object's name based on the original object's name. E.g.:
/// - "my object name" -> "my object name (1)"
pub fn get_duplicate_object_name(original_name: &str) -> String {
    match DUPLICATE_OBJECT_NAME_REGEX
        .captures(original_name)
        .and_then(|caps| caps.get(1))
        .and_then(|num| num.as_str().parse::<usize>().ok())
    {
        Some(num) => {
            let new_num = num.saturating_add(1);

            // edge case check for when the duplicate number is usize::MAX
            if new_num == usize::MAX {
                format!("{original_name} (1)")
            } else {
                DUPLICATE_OBJECT_NAME_REGEX
                    .replace(original_name, format!(" ({new_num})"))
                    .to_string()
            }
        }
        None => format!("{original_name} (1)"),
    }
}

impl Entity for UpdateManager {
    type Event = UpdateManagerEvent;
}

impl SingletonEntity for UpdateManager {}

#[cfg(test)]
#[path = "update_manager_tests.rs"]
mod tests;
