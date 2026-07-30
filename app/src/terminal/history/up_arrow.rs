use crate::settings::AISettings;
use std::collections::HashSet;

use warpui::{AppContext, EntityId, SingletonEntity};

use super::History;
use crate::input_suggestions::HistoryInputSuggestion;
use crate::suggestions::ignored_suggestions_model::{IgnoredSuggestionsModel, SuggestionType};
use crate::terminal::model::session::SessionId;

/// Controls which item types are included in up-arrow history results.
#[derive(Copy, Clone, Debug)]
pub struct UpArrowHistoryConfig {
    pub include_commands: bool,
    pub include_prompts: bool,
}

// LOCAL FORK: fn UpArrowHistoryConfig::for_input_config removed with the agent.

fn sort_and_dedupe_suggestions<'a>(
    mut suggestions: Vec<HistoryInputSuggestion<'a>>,
    session_id: Option<SessionId>,
    all_live_session_ids: &HashSet<SessionId>,
) -> Vec<HistoryInputSuggestion<'a>> {
    suggestions.sort_by(|a, b| a.cmp(b, session_id, all_live_session_ids));

    // Deduplicate commands and AI queries separately: keep the latest occurrence for each type.
    let mut seen_commands: HashSet<&str> = HashSet::new();
    let mut seen_ai_queries: HashSet<&str> = HashSet::new();
    let mut skip_indices: HashSet<usize> = HashSet::new();
    for (idx, suggestion) in suggestions.iter().enumerate().rev() {
        let text = suggestion.normalized_text();
        if text.is_empty() {
            skip_indices.insert(idx);
            continue;
        }
        if suggestion.is_ai_query() {
            if seen_ai_queries.contains(text) {
                skip_indices.insert(idx);
            } else {
                seen_ai_queries.insert(text);
            }
        } else if seen_commands.contains(text) {
            skip_indices.insert(idx);
        } else {
            seen_commands.insert(text);
        }
    }

    suggestions
        .into_iter()
        .enumerate()
        .filter(|(idx, _)| !skip_indices.contains(idx))
        .map(|(_, suggestion)| suggestion)
        .collect()
}
// LOCAL FORK: fn prompt_history_for_terminal_surface removed with the agent.

impl History {
    pub(crate) fn up_arrow_suggestions_for_terminal_surface<'a>(
        &'a self,
        _terminal_surface_id: EntityId,
        session_id: Option<SessionId>,
        config: UpArrowHistoryConfig,
        app: &'a AppContext,
    ) -> Vec<HistoryInputSuggestion<'a>> {
        // LOCAL FORK: prompt history went with the agent, so only shell commands
        // are ever offered here.
        if !config.include_commands {
            return vec![];
        }

        let ignored_suggestions = app
            .has_singleton_model::<IgnoredSuggestionsModel>()
            .then(|| IgnoredSuggestionsModel::handle(app).as_ref(app));

        let include_agent_commands = if app.has_singleton_model::<AISettings>() {
            *AISettings::handle(app)
                .as_ref(app)
                .include_agent_commands_in_history
        } else {
            true
        };

        let commands = session_id
            .and_then(|session_id| self.commands(session_id))
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| {
                ignored_suggestions.is_none_or(|ignored_suggestions| {
                    !ignored_suggestions.is_ignored(&entry.command, SuggestionType::ShellCommand)
                })
            })
            .filter(move |entry| include_agent_commands || !entry.is_agent_executed)
            .map(|entry| HistoryInputSuggestion::Command { entry });

        let all_live_session_ids = self.all_live_session_ids();
        sort_and_dedupe_suggestions(commands.collect(), session_id, &all_live_session_ids)
    }
}

#[cfg(test)]
#[path = "up_arrow_tests.rs"]
mod tests;
