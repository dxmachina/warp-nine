//! Warp UI Components module contains functions and structs that implement our internal components
//! used for the apps design (our buttons with styling, headers and panels etc.) as well definition
//! of colors (aka blended colors from the figma designs derived from Warp theme) and icons used
//! within the app.
pub(crate) mod avatar;
pub(crate) mod blended_colors;
pub(crate) mod breadcrumb;
pub mod buttons;
pub(crate) mod color_dot;
pub(crate) mod dialog;
pub(crate) mod icon_with_status;
pub(crate) mod item_highlight;
pub mod json_tree;
pub(crate) mod menu_button;
pub(crate) mod red_notification_dot;
pub(crate) mod render_file_search_row;
pub mod tab_selector;
// LOCAL FORK: rescued from ai/blocklist/block/. It is a generic keyboard-
// navigable button strip with no agent dependencies, used by the SSH server
// picker and the init-project and init-environment views, all of which stay.
pub mod keyboard_navigable_buttons;
pub mod inline_action;
// LOCAL FORK: rescued from ai/blocklist/block/. Generic toggle list widget,
// no agent dependencies; the LSP server selector is built on it.
pub mod toggleable_items;
pub(crate) mod window_focus_dimming;

pub use warp_core::ui::icons;

const BORDER_RADIUS: f32 = 4.;
