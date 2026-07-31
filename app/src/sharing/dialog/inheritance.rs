//! Support for displaying inherited ACLs.

use warp_core::ui::appearance::Appearance;
use warpui::elements::{CrossAxisAlignment, Flex, ParentElement as _};
use warpui::ui_components::components::UiComponent as _;
use warpui::{AppContext, Element, SingletonEntity as _};

use super::style;
use crate::cloud_object::ServerObjectContainer;
use crate::cloud_object::model::persistence::CloudModel;
use crate::server::ids::SyncId;

/// UI state for inherited permissions.
pub struct InheritanceState {
    // The server API allows inheriting ACLs from drives as well, but we currently don't use this.
    source_folder: SyncId,
}

impl InheritanceState {
    /// Construct inheritance state for an object and the source of its possibly-inherited ACL.
    pub fn from_object_and_source(
        object_id: &SyncId,
        source: Option<&ServerObjectContainer>,
    ) -> Option<InheritanceState> {
        let source_folder = match source? {
            ServerObjectContainer::Folder { folder_uid } => SyncId::ServerId(*folder_uid),
            _ => return None,
        };
        // ACLs _on_ folders may include themselves as sources.
        if &source_folder == object_id {
            return None;
        }

        Some(InheritanceState { source_folder })
    }

    pub fn details(&self, appearance: &Appearance, app: &AppContext) -> InheritanceDetails {
        let folder_name = CloudModel::as_ref(app)
            .get_folder(&self.source_folder)
            .map(|folder| &folder.model().name);

        match folder_name {
            // LOCAL FORK: the folder name used to be a link that opened the parent folder's own
            // sharing settings in the Warp Drive index. The index went with the Warp Drive
            // browser and folders have no UI left, so the name is now plain text and the tooltip
            // says what is actually true.
            Some(folder_name) => {
                let prefix = style::detail_text("Inherited from ", appearance)
                    .build()
                    .finish();
                let folder_name = style::detail_text(folder_name.to_owned(), appearance)
                    .build()
                    .finish();

                InheritanceDetails {
                    source_label: Flex::row()
                        .with_children([prefix, folder_name])
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .finish(),
                    tooltip_text: "Cannot edit inherited permissions",
                }
            }
            None => InheritanceDetails {
                source_label: style::detail_text("Inherited permission", appearance)
                    .build()
                    .finish(),
                tooltip_text: "Cannot edit inherited permissions",
            },
        }
    }
}

/// Information to display about inherited permissions.
pub struct InheritanceDetails {
    /// A label element describing where an ACL was inherited from, with a link to edit those
    /// permissions directly.
    pub source_label: Box<dyn Element>,
    /// A tooltip to show on disabled permission-editing controls.
    pub tooltip_text: &'static str,
}
