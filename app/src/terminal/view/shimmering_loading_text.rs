//! Shimmering loading text with the Warp glyph.
//!
//! LOCAL FORK: this originally lived in the deleted `ai::loading` module. It has no
//! agent dependencies and the SSH remote-server loading footer still renders it, so it
//! was rescued into its own module here.

use warpui::elements::shimmering_text::{
    ShimmerConfig, ShimmeringTextElement, ShimmeringTextStateHandle,
};
use warpui::{AppContext, Element, SingletonEntity as _};

use crate::appearance::Appearance;

/// Warp icon glyph character.
const WARP_GLYPH: &str = "\u{E500}";

/// Creates a shimmering text element with the Warp glyph.
pub(crate) fn shimmering_warp_loading_text(
    text: impl Into<String>,
    font_size: f32,
    shimmer_handle: ShimmeringTextStateHandle,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();
    let base_color = theme.disabled_text_color(theme.surface_1()).into_solid();
    let shimmer_color = theme.main_text_color(theme.surface_1()).into_solid();

    ShimmeringTextElement::new(
        format!("{} {}", WARP_GLYPH, text.into()),
        appearance.ui_font_family(),
        font_size,
        base_color,
        shimmer_color,
        ShimmerConfig::default(),
        shimmer_handle,
    )
    .finish()
}
