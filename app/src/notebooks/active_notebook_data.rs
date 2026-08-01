use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use super::{CloudNotebookModel, NotebookId};
use crate::cloud_object::model::persistence::{CloudModel, CloudModelEvent};
use crate::cloud_object::model::view::{CloudViewModel, Editor, EditorState};
use crate::cloud_object::{CloudObject, Owner, Space};
use crate::notebooks::CloudNotebook;
use crate::server::cloud_objects::update_manager::{
    ObjectOperation, OperationSuccessType, UpdateManager, UpdateManagerEvent,
};
use crate::server::ids::{ClientId, SyncId};
use crate::sharing::{ContentEditability, SharingAccessLevel};

#[derive(Default, Clone)]
pub enum ActiveNotebook {
    #[default]
    None,
    // A notebook already stored in CloudModel, all relevant data should be queried
    // from CloudModel directly
    CommittedNotebook(SyncId),
    // A notebook that has been created and displayed in the view, but is not yet
    // committed to CloudModel
    NewNotebook(Box<CloudNotebook>),
}

#[derive(PartialEq, Eq, Default, Clone, Copy, Debug)]
pub enum Mode {
    #[default]
    Editing,
    View,
}

/// Data displayed in the status bar that is also relevant for workflows and notebooks.
/// We share this data between views by making it a model.
#[derive(Default)]
pub struct ActiveNotebookData {
    /// Whether we're in editing, readonly or viewing mode.
    pub mode: Mode,
    pub active_notebook: ActiveNotebook,

    pub show_grab_edit_access_modal: bool,
}

impl ActiveNotebookData {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let update_manager = UpdateManager::handle(ctx);

        ctx.subscribe_to_model(&update_manager, |me, _, event, ctx| {
            me.handle_update_manager_event(event, ctx);
        });

        let cloud_model = CloudModel::handle(ctx);
        ctx.subscribe_to_model(&cloud_model, |me, _, event, ctx| {
            me.handle_cloud_model_event(event, ctx);
        });

        Self {
            ..Default::default()
        }
    }

    fn handle_cloud_model_event(&mut self, event: &CloudModelEvent, ctx: &mut ModelContext<Self>) {
        match event {
            CloudModelEvent::NotebookEditorChangedFromServer { notebook_id } => {
                if self.is_active_notebook(*notebook_id) {
                    if let Some(new_editor) =
                        CloudViewModel::as_ref(ctx).object_current_editor(&notebook_id.uid(), ctx)
                        && self.mode == Mode::Editing
                        && matches!(new_editor.state, EditorState::OtherUserActive)
                    {
                        self.mode = Mode::View;
                        ctx.emit(ActiveNotebookDataEvent::ModeChangedFromServer);
                    }
                    ctx.notify();
                }
            }
            CloudModelEvent::ObjectMoved { type_and_id, .. } => {
                if let Some(notebook_id) = type_and_id.as_notebook_id() {
                    // Update breadcrumb when a notebook is moved, whether by the user or a
                    // teammate.
                    if self.is_active_notebook(notebook_id) {
                        ctx.emit(ActiveNotebookDataEvent::BreadcrumbsChanged);
                    }
                }
            }
            _ => (),
        }
    }

    fn handle_update_manager_event(
        &mut self,
        event: &UpdateManagerEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        let UpdateManagerEvent::ObjectOperationComplete { result } = event else {
            return;
        };

        // LOCAL FORK: only trashing, untrashing and taking the edit baton still produce a
        // result, and only ever a successful one. The `Create`, `Update`, `Rejection` and
        // `FeatureNotAvailable` arms all read a server's answer to a request: a create
        // reported the server id that had just been minted for a client id, an update
        // reported that the write landed, a rejection that the revision was stale, and
        // `FeatureNotAvailable` that the workspace was not entitled to notebooks.
        //
        // The match is on the object's `SyncId` rather than a `ServerId`, because these
        // operations now run on objects that have only ever had a client id.
        match (&result.operation, &result.success_type) {
            (ObjectOperation::TakeEditAccess, OperationSuccessType::Success) => {
                if self.id().is_some() && self.id() == result.object_id {
                    self.mode = Mode::Editing;
                    ctx.emit(ActiveNotebookDataEvent::SwitchedToEditMode);
                }
            }
            (ObjectOperation::Trash, OperationSuccessType::Success)
            | (ObjectOperation::Untrash, OperationSuccessType::Success) => {
                if self.id().is_some() && self.id() == result.object_id {
                    ctx.emit(ActiveNotebookDataEvent::TrashStatusChanged);
                }
            }
            _ => {}
        }
    }

    pub fn reset(&mut self) {
        self.mode = Mode::View;
        self.show_grab_edit_access_modal = false;
        self.active_notebook = ActiveNotebook::None;
    }

    pub fn open_new(
        &mut self,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.reset();

        // create a new client id
        let new_id = ClientId::default();

        // Set the active notebook to be an uncommitted notebook
        self.active_notebook = ActiveNotebook::NewNotebook(Box::new(CloudNotebook::new_local(
            CloudNotebookModel::default(),
            owner,
            initial_folder_id,
            new_id,
        )));
        ctx.emit(ActiveNotebookDataEvent::BreadcrumbsChanged);
    }

    pub fn open_existing(&mut self, notebook_id: SyncId, ctx: &mut ModelContext<Self>) {
        self.reset();
        self.active_notebook = ActiveNotebook::CommittedNotebook(notebook_id);
        ctx.emit(ActiveNotebookDataEvent::BreadcrumbsChanged);
    }

    pub fn id(&self) -> Option<SyncId> {
        match &self.active_notebook {
            ActiveNotebook::None => None,
            ActiveNotebook::CommittedNotebook(id) => Some(*id),
            ActiveNotebook::NewNotebook(notebook) => Some(notebook.id),
        }
    }

    pub fn active_notebook(&self) -> ActiveNotebook {
        self.active_notebook.clone()
    }

    /// Whether or not the notebook has been synced to the server.
    pub fn is_on_server(&self) -> bool {
        matches!(
            &self.active_notebook,
            ActiveNotebook::CommittedNotebook(SyncId::ServerId(_))
        )
    }

    // LOCAL FORK: `breadcrumbs` fed the location row that went with the Warp Drive
    // browser. `containing_objects_path` itself stays: `CloudObject::breadcrumbs()`
    // still renders the "Personal / folder" line on ~20 command-palette and search
    // items.

    /// The space that the active notebook is shown in for this user.
    pub fn space(&self, app: &AppContext) -> Option<Space> {
        match &self.active_notebook {
            ActiveNotebook::None => None,
            ActiveNotebook::CommittedNotebook(id) => CloudModel::as_ref(app)
                .get_notebook(id)
                .map(|notebook| notebook.space(app)),
            ActiveNotebook::NewNotebook(notebook) => Some(notebook.space(app)),
        }
    }

    /// The drive that owns the active notebook.
    pub fn owner(&self, app: &AppContext) -> Option<Owner> {
        match &self.active_notebook {
            ActiveNotebook::None => None,
            ActiveNotebook::CommittedNotebook(id) => CloudModel::as_ref(app)
                .get_notebook(id)
                .map(|notebook| notebook.permissions.owner),
            ActiveNotebook::NewNotebook(notebook) => Some(notebook.permissions.owner),
        }
    }

    pub fn is_active_notebook(&self, notebook_id: SyncId) -> bool {
        self.id() == Some(notebook_id)
    }

    /// Checks whether or not this notebook has edit conflicts that would
    /// results in the conflict resolution banner being shown. We check both
    /// if a conflicting object has been received from the server, and that there
    /// are no pending content changes on the notebook.
    ///
    /// We need to check the pending content changes because of a race condition where
    /// echo'd back RTC messages can come in before a server response and incorrectly apply
    /// a conflict to the notebook. To ensure we don't incorrectly show the dialog, we wait until
    /// all pending requests have returned.
    pub fn has_conflicts(&self, ctx: &AppContext) -> bool {
        self.id()
            .and_then(|id| CloudModel::as_ref(ctx).get_by_uid(&id.uid()))
            .is_some_and(|object| {
                object.has_conflicting_changes() && !object.metadata().has_pending_content_changes()
            })
    }

    /// Returns the current editor of the active object. Returns None
    /// if there is not currently an active notebook
    pub fn current_editor(&self, ctx: &AppContext) -> Option<Editor> {
        let id = self.id()?;
        CloudViewModel::as_ref(ctx).object_current_editor(&id.uid(), ctx)
    }

    /// Checks if this notebook is trashed or deleted.
    pub fn trash_status(&self, ctx: &AppContext) -> TrashStatus {
        match &self.active_notebook {
            ActiveNotebook::None | ActiveNotebook::NewNotebook(_) => TrashStatus::Active,
            ActiveNotebook::CommittedNotebook(id) => {
                let cloud_model = CloudModel::as_ref(ctx);
                match cloud_model.get_notebook(id) {
                    Some(notebook) => {
                        if notebook.is_trashed(cloud_model) {
                            TrashStatus::Trashed
                        } else {
                            TrashStatus::Active
                        }
                    }
                    None => TrashStatus::Deleted,
                }
            }
        }
    }

    /// The current user's access level on the notebook.
    pub fn access_level(&self, app: &AppContext) -> SharingAccessLevel {
        match &self.active_notebook {
            ActiveNotebook::CommittedNotebook(object_id) => {
                CloudViewModel::as_ref(app).access_level(&object_id.uid(), app)
            }
            ActiveNotebook::None | ActiveNotebook::NewNotebook(_) => SharingAccessLevel::Full,
        }
    }

    /// Whether or not the current user can edit the notebook.
    pub fn editability(&self, app: &AppContext) -> ContentEditability {
        match &self.active_notebook {
            ActiveNotebook::CommittedNotebook(object_id) => {
                CloudViewModel::as_ref(app).object_editability(&object_id.uid(), app)
            }
            ActiveNotebook::None | ActiveNotebook::NewNotebook(_) => ContentEditability::Editable,
        }
    }
}

pub enum ActiveNotebookDataEvent {
    /// Another user stole the baton for the current object.
    ModeChangedFromServer,
    /// The editing baton for the current object was successfully grabbed server-side.
    SwitchedToEditMode,
    /// The notebook's breadcrumbs were updated.
    BreadcrumbsChanged,
    /// This notebook was trashed or untrashed (used for refreshing pane overflow items)
    TrashStatusChanged,
    // This notebook was moved to a shared space.
    MovedToSpace,
}

/// Whether or not a notebook is trashed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrashStatus {
    Active,
    Trashed,
    Deleted,
}

impl TrashStatus {
    /// Whether or not the notebook can be edited in this state.
    pub fn is_editable(self) -> bool {
        match self {
            TrashStatus::Active => true,
            TrashStatus::Trashed | TrashStatus::Deleted => false,
        }
    }
}

impl Entity for ActiveNotebookData {
    type Event = ActiveNotebookDataEvent;
}
