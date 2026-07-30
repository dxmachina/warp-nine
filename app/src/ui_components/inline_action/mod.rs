// LOCAL FORK: rescued from ai/blocklist/inline_action/.
//
// The header chrome and its status icons are generic inline-block widgets with
// no agent dependencies. They render the init-project and init-environment
// setup steps and the LSP server selector, all of which this fork keeps.
// Only the surrounding agent action model was deleted.

pub mod inline_action_header;
pub mod inline_action_icons;
// LOCAL FORK: rescued from ai/agent/icons.rs. Status glyphs only.
pub mod status_icons;
