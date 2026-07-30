use warpui::ViewContext;

use super::TerminalView;
use crate::server::telemetry::InteractionSource;
use crate::terminal::view::CodeDiffAction;

#[derive(Copy, Clone, Debug)]
pub enum PromptSuggestionResolution {
    Accept {
        interaction_source: InteractionSource,
    },
    Reject {
        ctrl_c: bool,
    },
}

impl From<PromptSuggestionResolution> for CodeDiffAction {
    fn from(value: PromptSuggestionResolution) -> Self {
        match value {
            PromptSuggestionResolution::Accept { .. } => CodeDiffAction::Accept,
            PromptSuggestionResolution::Reject { .. } => CodeDiffAction::Reject,
        }
    }
}

impl TerminalView {
    /// LOCAL FORK: every passive suggestion (code diff, unit test, prompt) was produced
    /// by an AI block, so there is never one to resolve. Kept as a no-op because the
    /// `ResolvePromptSuggestion` action and the ctrl-c path still call it.
    pub(super) fn resolve_passive_suggestion(
        &mut self,
        _resolution: PromptSuggestionResolution,
        _ctx: &mut ViewContext<Self>,
    ) -> bool {
        false
    }
}
