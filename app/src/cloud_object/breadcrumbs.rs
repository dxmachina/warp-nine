use warpui::AppContext;

use super::{CloudObject, Space};
use crate::cloud_object::folders::CloudFolder;

// Encapsulates an object that can contain other objects, and keeps
// information necessary to describe where an object lives.
//
// LOCAL FORK: this also carried a `kind: ContainingObjectKind` holding the container's `Space` or
// `CloudObjectTypeAndId`, plus `ContainingObjectKind::into_item_id`, which turned that into a
// `WarpDriveItemId`. It existed only so the `ViewInWarpDrive` action could select the container in
// the Warp Drive panel. The action and the breadcrumb rows that dispatched it went with the panel,
// leaving the field with no readers, so the field and the enum went too. `name` is still read by
// `CloudObject::containing_object_name` and `CloudObject::breadcrumbs`, which the command palette
// and the search surfaces use to show an object's location.
#[derive(Clone, Debug)]
pub struct ContainingObject {
    pub name: String,
}

impl From<&CloudFolder> for ContainingObject {
    fn from(folder: &CloudFolder) -> Self {
        Self {
            name: folder.display_name().clone(),
        }
    }
}

impl Space {
    pub fn into_containing_object(self, app: &AppContext) -> ContainingObject {
        ContainingObject {
            name: self.name(app).clone(),
        }
    }
}
