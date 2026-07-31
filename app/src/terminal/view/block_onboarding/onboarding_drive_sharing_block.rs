use warp_core::ui::appearance::Appearance;
use warpui::elements::{Border, Container, Flex, ParentElement, Text};
use warpui::fonts::{Properties, Weight};
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::{AppContext, Element, Entity, SingletonEntity, View, ViewContext};

use crate::cloud_object::CloudObjectTypeAndId;
use crate::cloud_object::model::persistence::{CloudModel, CloudModelEvent};

/// A rich onboarding block that tells the user how to share a newly-created personal cloud
/// object.
// LOCAL FORK: the block used to end in a "Share <object>" button that dispatched
// `WorkspaceAction::OpenObjectSharingSettings`, which opened the Warp Drive index's share
// dialog. The index went with the Warp Drive browser, so the button went with it and the copy
// now points only at the pane header, which owns a working share dialog of its own.
pub struct OnboardingDriveSharingBlock {
    object_id: CloudObjectTypeAndId,
}

impl OnboardingDriveSharingBlock {
    pub fn new(object_id: CloudObjectTypeAndId, ctx: &mut ViewContext<Self>) -> Self {
        // Re-render if the object in the block is renamed.
        ctx.subscribe_to_model(&CloudModel::handle(ctx), |me, _, event, ctx| {
            if let CloudModelEvent::ObjectUpdated { type_and_id, .. } = event
                && &me.object_id == type_and_id
            {
                ctx.notify();
            }
        });

        Self { object_id }
    }
}

impl Entity for OnboardingDriveSharingBlock {
    type Event = ();
}

const TITLE_TEXT: &str = "Sharing in Warp Drive";
const BODY_TEXT: &[&str] = &[
    "You can now share saved objects, in Warp or on the web, with anyone - Warp user or not. Click Share in the pane header to share via link or email.",
    "You’ll be able to modify the access permissions any time.",
];

const BLOCK_PADDING: f32 = 16.;

impl View for OnboardingDriveSharingBlock {
    fn ui_name() -> &'static str {
        "OnboardingDriveSharingBlock"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let font_family = appearance.monospace_font_family();
        let font_size = appearance.monospace_font_size();

        let header = Container::new(
            Text::new(TITLE_TEXT, font_family, font_size)
                .with_color(appearance.theme().accent().into_solid())
                .with_style(Properties::default().weight(Weight::Bold))
                .finish(),
        )
        .with_padding_bottom(BLOCK_PADDING)
        .finish();

        let mut content = Flex::column().with_child(header);

        for paragraph in BODY_TEXT.iter() {
            content.add_child(
                appearance
                    .ui_builder()
                    .paragraph(*paragraph)
                    .with_style(UiComponentStyles {
                        font_family_id: Some(font_family),
                        font_size: Some(font_size),
                        ..Default::default()
                    })
                    .build()
                    .with_padding_bottom(BLOCK_PADDING)
                    .finish(),
            );
        }

        Container::new(content.finish())
            .with_uniform_padding(BLOCK_PADDING)
            .with_border(Border::top(1.).with_border_fill(appearance.theme().outline()))
            .finish()
    }
}
