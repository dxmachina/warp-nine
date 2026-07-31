//! Arguments for opening a cloud object.
//!
//! LOCAL FORK: lifted out of `app/src/drive/mod.rs`. `OpenWarpDriveObjectSettings` sits in the
//! pane restore path -- `persistence/sqlite.rs` builds `NotebookPaneSnapshot::CloudNotebook` and
//! `WorkflowPaneSnapshot::CloudWorkflow` with it -- so it long outlives the browser.
//!
//! `focused_folder_id` is vestigial now that no panel can focus a folder. It is left in place so
//! this stays a move rather than a behaviour change; removing it is a follow-up.

use crate::cloud_object::ObjectType;
use crate::server::ids::ServerId;

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct OpenWarpDriveObjectSettings {
    /// The folder that should be focused in the Warp Drive when the object is opened.
    pub focused_folder_id: Option<ServerId>,
    /// The email of the user to invite to the object, if the object is being opened via the request access flow.
    pub invitee_email: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct OpenWarpDriveObjectArgs {
    pub object_type: ObjectType,
    pub server_id: ServerId,
    pub settings: OpenWarpDriveObjectSettings,
}
