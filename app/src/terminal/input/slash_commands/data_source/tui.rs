use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::FairMutex;
#[cfg(feature = "voice_input")]
use warpui::SingletonEntity as _;
use warpui::{AppContext, Entity, EntityId, ModelContext, ModelHandle};

use super::core::subscribe_to_shared_dependencies;
use super::{
    InlineItem, SlashCommandDataSource, SlashCommandDataSourceState, UpdatedActiveCommands,
};
use crate::search::SyncDataSource;
use crate::search::data_source::{Query, QueryResult};
use crate::search::mixer::DataSourceRunErrorWrapper;
use crate::search::slash_command_menu::static_commands::Availability;
use crate::search::slash_command_menu::static_commands::commands::{COMMAND_REGISTRY, VOICE};
use crate::terminal::TerminalModel;
use crate::terminal::input::slash_commands::AcceptSlashCommandOrSavedPrompt;
use crate::terminal::model::session::active_session::ActiveSession;
// LOCAL FORK: these four voice_input gates were dangling. The excision deleted
// `use crate::ai::{AIRequestUsageModel, ...}` but left its `#[cfg]` behind, so the
// attribute slid onto the next import and gated `SyncDataSource` and
// `TerminalModel`, both of which are used unconditionally. That broke every
// build without `voice_input`; the bundle hid it because `gui = ["voice_input"]`.
#[cfg(feature = "voice_input")]
use crate::settings::{AISettings, AISettingsChangedEvent};
#[cfg(feature = "voice_input")]
use crate::workspaces::user_workspaces::{UserWorkspaces, UserWorkspacesEvent};

pub struct TuiDataSourceArgs {
    pub active_session: ModelHandle<ActiveSession>,
    pub terminal_view_id: EntityId,
    pub terminal_model: Arc<FairMutex<TerminalModel>>,
}

pub struct TuiSlashCommandDataSource {
    state: SlashCommandDataSourceState,
    terminal_model: Arc<FairMutex<TerminalModel>>,
}

impl TuiSlashCommandDataSource {
    pub fn new(args: TuiDataSourceArgs, ctx: &mut ModelContext<Self>) -> Self {
        let TuiDataSourceArgs {
            active_session,
            terminal_view_id,
            terminal_model,
        } = args;

        subscribe_to_shared_dependencies(
            &active_session,
            terminal_view_id,
            Self::recompute_active_commands,
            ctx,
        );

        #[cfg(feature = "voice_input")]
        {
            ctx.subscribe_to_model(&AISettings::handle(ctx), |me, _, event, ctx| {
                if matches!(event, AISettingsChangedEvent::VoiceInputEnabled { .. }) {
                    me.recompute_active_commands(ctx);
                }
            });
            ctx.subscribe_to_model(&UserWorkspaces::handle(ctx), |me, _, event, ctx| {
                if matches!(event, UserWorkspacesEvent::TeamsChanged) {
                    me.recompute_active_commands(ctx);
                }
            });
            // LOCAL FORK: the AI request-usage subscription went with the agent.
        }

        let mut me = Self {
            state: SlashCommandDataSourceState::new(active_session, terminal_view_id),
            terminal_model,
        };
        me.recompute_active_commands(ctx);
        me
    }

    /// LOCAL FORK: AI query routing went with the agent, and skills went with
    /// it, so nothing is ever served locally.
    pub fn local_skills_available(&self, _app: &AppContext) -> bool {
        false
    }
    pub fn set_active_repo_root(
        &mut self,
        repo_root: Option<PathBuf>,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.update_active_repo_root(repo_root) {
            self.recompute_active_commands(ctx);
        }
    }

    fn recompute_active_commands(&mut self, ctx: &mut ModelContext<Self>) {
        let availability = self.availability(ctx);
        let gates = self.common_command_gates(ctx);
        // LOCAL FORK: the voice credit check went with the agent's request-usage model.
        #[cfg(feature = "voice_input")]
        let voice_command_is_available = AISettings::as_ref(ctx).is_voice_input_enabled(ctx)
            && UserWorkspaces::as_ref(ctx).is_voice_enabled()
            && self.local_skills_available(ctx);
        #[cfg(not(feature = "voice_input"))]
        let voice_command_is_available = false;
        let commands = HashMap::from_iter(
            COMMAND_REGISTRY
                .all_commands_by_id()
                .filter(|(_, command)| {
                    (command.name != VOICE.name || voice_command_is_available)
                        && self.command_passes_common_gates(command, availability, &gates)
                })
                .map(|(id, command)| (id, command.clone())),
        );
        if self.replace_active_commands(commands) {
            ctx.emit(UpdatedActiveCommands);
        }
    }

    fn availability(&self, ctx: &AppContext) -> Availability {
        self.base_availability(ctx)
            | Availability::AGENT_VIEW
            | Availability::ACTIVE_CONVERSATION
            | Availability::NOT_CLOUD_AGENT
    }
}

impl SyncDataSource for TuiSlashCommandDataSource {
    type Action = AcceptSlashCommandOrSavedPrompt;

    fn run_query(
        &self,
        query: &Query,
        app: &AppContext,
    ) -> Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper> {
        if query.text.is_empty() {
            return Ok(vec![]);
        }

        let query_text = query.text.trim().to_lowercase();
        let mut results = self.match_active_commands(&query_text, app);
        if self.local_skills_available(app) {
            results.extend(self.match_skills(&query_text, app));
        }
        Ok(results
            .into_iter()
            .map(|item: InlineItem| item.into())
            .collect())
    }
}

impl SlashCommandDataSource for TuiSlashCommandDataSource {
    fn state(&self) -> &SlashCommandDataSourceState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut SlashCommandDataSourceState {
        &mut self.state
    }
}

impl Entity for TuiSlashCommandDataSource {
    type Event = UpdatedActiveCommands;
}
