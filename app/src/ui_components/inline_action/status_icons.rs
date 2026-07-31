use warp_core::ui::appearance::Appearance;
use warp_core::ui::theme::AnsiColorIdentifier;

use crate::ui_components::icons::Icon;

// LOCAL FORK: the agent-status icons (todo_list, pending, succeeded,
// addressed_comment, failed, gray_stop, gray_clock, gray_circle, red_stop) had
// no callers left once the agent block UI went. The three below are used by the
// init-environment / init-project views and the env-var collection block.

pub fn in_progress_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(
        Icon::Circle.into(),
        AnsiColorIdentifier::Magenta.to_ansi_color(&appearance.theme().terminal_colors().normal),
    )
}

/// Not running, requires user's attention
pub fn yellow_stop_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(
        Icon::StopFilled.into(),
        AnsiColorIdentifier::Yellow.to_ansi_color(&appearance.theme().terminal_colors().normal),
    )
}

/// To be used for actions (like running commands/reading files) that are long-running and executing.
pub fn yellow_running_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(
        Icon::Circle.into(),
        AnsiColorIdentifier::Yellow.to_ansi_color(&appearance.theme().terminal_colors().normal),
    )
}
