//! LOCAL FORK: rescued from `ai/blocklist/block/numbered_button.rs`.
//!
//! The original module built the numbered option buttons for agent suggestion
//! blocks. Everything except [`render_recommended_badge`] existed only to serve
//! those blocks (and `build_inline_input_content` took a `CompactAgentInput`
//! handle directly), so only the badge is kept here. It has no agent
//! dependencies and is used by
//! [`crate::ui_components::keyboard_navigable_buttons::rich_navigation_button`].

use warp_core::ui::appearance::Appearance;
use warp_core::ui::theme::color::internal_colors;
use warpui::Element;
use warpui::elements::{Container, CornerRadius, Radius, Text};

use crate::context_chips::spacing;

/// A small muted "Recommended" chip, rendered next to the title of the option
/// the UI wants to steer the user toward.
pub(super) fn render_recommended_badge(appearance: &Appearance) -> Box<dyn Element> {
    let theme = appearance.theme();
    Container::new(
        Text::new(
            "Recommended".to_string(),
            appearance.ui_font_family(),
            appearance.monospace_font_size() - 2.,
        )
        .with_color(internal_colors::neutral_6(theme))
        .finish(),
    )
    .with_background(internal_colors::fg_overlay_2(theme))
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
    .with_vertical_padding(spacing::UDI_CHIP_VERTICAL_PADDING)
    .with_horizontal_padding(spacing::UDI_CHIP_HORIZONTAL_PADDING)
    .finish()
}
