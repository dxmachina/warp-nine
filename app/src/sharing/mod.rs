//! LOCAL FORK: what remains of the sharing module after session sharing was removed.
//!
//! This was two things: the Warp Drive ACL model (already reduced to a re-export when
//! drive sharing went) and the shared-session sharing dialog. With the dialog gone the
//! only survivors are the `cloud_objects` re-exports and [`ContentEditability`], which
//! the notebook, workflow and env-var views use to decide whether their contents are
//! editable. Those views are local-only now, so the answer is always
//! [`ContentEditability::Editable`], but the type is what their signatures speak and
//! collapsing it would touch far more code than keeping it.
//!
//! Gone with the dialog: `ShareableObject`, `SubjectExt`, `UserKindExt`, the QR code
//! renderer and the dialog's style module. Nothing outside the dialog used them.

// Re-export types from cloud_objects.
pub use cloud_objects::drive::sharing::{
    LinkSharingSubjectType, SharingAccessLevel, Subject, TeamKind, UserKind,
};

/// Whether not a shared object's contents are editable by the current user.
///
/// This is not purely a function of their access level since anonymous users are not allowed to
/// edit (due to the lack of attribution).
#[derive(Debug, Clone, Copy)]
pub enum ContentEditability {
    ReadOnly,
    RequiresLogin,
    Editable,
}

impl ContentEditability {
    pub fn can_edit(self) -> bool {
        matches!(self, ContentEditability::Editable)
    }
}
