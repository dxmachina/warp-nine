use std::collections::HashMap;

use chrono::{DateTime, Utc};
pub use cloud_object_models::*;
pub use cloud_objects::cloud_object::*;
use cloud_objects::ids::{
    FolderId, GenericStringObjectId, HashedSqliteId, ObjectUid, ServerId, SyncId,
};
use warp_graphql::mcp_gallery_template::MCPGalleryTemplate;

/// Identifies a guest to remove from an object.
#[derive(Clone, Debug)]
pub enum GuestIdentifier {
    /// Remove a user guest by their email address.
    Email(String),
    /// Remove a team guest by their team UID.
    TeamUid(ServerId),
}

/// The type of action that occurred on an object, such as an execution, selection, so on
/// and so forth.
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectActionType {
    Execute,
}

// In order to convert from a graphql type and from a SQLite read, the action type
// implements to_string().
//
// Temporarily suppress clippy warnings about the `ToString` impl until we
// move `ObjectType` away from using `std::fmt::Display` for serialization.
#[allow(clippy::to_string_trait_impl)]
impl ToString for ObjectActionType {
    fn to_string(&self) -> String {
        match self {
            ObjectActionType::Execute => String::from("EXECUTE"),
        }
    }
}

impl ObjectActionType {
    pub fn singular(&self) -> String {
        match self {
            ObjectActionType::Execute => "run".to_string(),
        }
    }

    pub fn plural(&self) -> String {
        match self {
            ObjectActionType::Execute => "runs".to_string(),
        }
    }
}

/// We track object actions, both those that have been sent to the server and not, through this
/// type. A single ObjectAction represents an object_id, action pair and a subtype that contains data
/// about the action(s). Each ObjectAction either represents one action or a summary of identical actions
/// that occurred at different times. We summarize old actions in order to save memory footprint on the client.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectAction {
    pub action_type: ObjectActionType,
    pub uid: ObjectUid,
    pub hashed_sqlite_id: HashedSqliteId,
    // This action either represents one action or a consolidation of multiple actions.
    pub action_subtype: ObjectActionSubtype,
}

impl ObjectAction {
    pub fn is_pending(&self) -> bool {
        match self.action_subtype {
            ObjectActionSubtype::SingleAction { pending, .. } => pending,
            ObjectActionSubtype::BundledActions { .. } => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectActionHistory {
    pub uid: ObjectUid,
    pub hashed_sqlite_id: HashedSqliteId,
    pub latest_processed_at_timestamp: DateTime<Utc>,
    pub actions: Vec<ObjectAction>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjectActionSubtype {
    SingleAction {
        timestamp: DateTime<Utc>,
        processed_at_timestamp: Option<DateTime<Utc>>,
        data: Option<String>,
        pending: bool,
    },
    BundledActions {
        count: i32,
        oldest_timestamp: DateTime<Utc>,
        latest_timestamp: DateTime<Utc>,
        latest_processed_at_timestamp: DateTime<Utc>,
    },
}

#[derive(Default)]
pub struct InitialLoadResponse {
    pub updated_notebooks: Vec<ServerNotebook>,
    pub deleted_notebooks: Vec<cloud_object_models::NotebookId>,
    pub updated_workflows: Vec<ServerWorkflow>,
    pub deleted_workflows: Vec<WorkflowId>,
    pub updated_folders: Vec<ServerFolder>,
    pub deleted_folders: Vec<FolderId>,
    pub updated_generic_string_objects:
        HashMap<GenericStringObjectFormat, Vec<Box<dyn ServerObject>>>,
    pub deleted_generic_string_objects: Vec<GenericStringObjectId>,
    pub user_profiles: Vec<UserProfileWithUID>,
    pub action_histories: Vec<ObjectActionHistory>,
    pub mcp_gallery: Vec<MCPGalleryTemplate>,
}

pub struct GetCloudObjectResponse {
    pub object: ServerCloudObject,
    pub descendants: Vec<ServerCloudObject>,
    pub action_histories: Vec<ObjectActionHistory>,
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum ObjectUpdateMessage {
    ObjectMetadataChanged {
        metadata: ServerMetadata,
    },
    ObjectPermissionsChanged,
    ObjectPermissionsChangedV2 {
        object_uid: ServerId,
        permissions: ServerPermissions,
        user_profiles: Vec<UserProfileWithUID>,
    },
    ObjectContentChanged {
        server_object: Box<ServerCloudObject>,
        last_editor: Option<UserProfileWithUID>,
    },
    ObjectDeleted {
        object_uid: ServerId,
    },
    ObjectActionOccurred {
        history: ObjectActionHistory,
    },
    TeamMembershipsChanged,
    AmbientTaskUpdated {
        task_id: String,
        timestamp: DateTime<Utc>,
    },
}

impl ObjectUpdateMessage {
    pub fn as_str(&self) -> &'static str {
        use ObjectUpdateMessage::*;
        match self {
            ObjectMetadataChanged { .. } => "ObjectMetadataChanged",
            ObjectPermissionsChanged => "ObjectPermissionsChanged",
            ObjectPermissionsChangedV2 { .. } => "ObjectPermissionsChanged (V2)",
            ObjectContentChanged { .. } => "ObjectContentChanged",
            ObjectDeleted { .. } => "ObjectDeleted",
            ObjectActionOccurred { .. } => "ObjectActionOccurred",
            TeamMembershipsChanged => "TeamMembershipsChanged",
            AmbientTaskUpdated { .. } => "AmbientTaskUpdated",
        }
    }
}

#[derive(Clone, Debug)]
pub enum ObjectPermissionUpdateResult {
    Success,
    Failure,
}

#[derive(Clone, Debug)]
pub struct ObjectPermissionsUpdateData {
    pub permissions: ServerPermissions,
    pub profiles: Vec<UserProfileWithUID>,
}

#[derive(Clone, Debug)]
pub enum ObjectMetadataUpdateResult {
    Success { metadata: Box<ServerMetadata> },
    Failure,
}

pub enum ObjectDeleteResult {
    Success { deleted_ids: Vec<SyncId> },
    Failure,
}

// LOCAL FORK: the `ObjectClient` trait stood here, and with it the request and response
// shapes only it used. It described the whole cloud-object API surface -- create, update,
// fetch, the initial and incremental loads, move, trash, untrash, delete, empty trash, the
// notebook edit baton and the guest and link-sharing calls -- and `automock` generated a
// `MockObjectClient` from it for tests. Its one implementation was `server_api::object`,
// which is gone, and nothing constructs a client any more.
//
// Everything above stays. Those types describe cloud objects and the events that change
// them, which the local model still uses; only the transport went.
