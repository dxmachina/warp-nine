use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(not(target_family = "wasm"))]
use warp_cli::agent::Harness;
use warp_core::features::FeatureFlag;
use warpui::{AppContext, Entity, EntityId, ModelContext, ModelHandle, SingletonEntity};

use super::core::subscribe_to_shared_dependencies;
use super::{
    InlineItem, SlashCommandDataSource, SlashCommandDataSourceState, UpdatedActiveCommands,
};
use crate::search::SyncDataSource;
use crate::search::data_source::{Query, QueryResult};
use crate::search::mixer::DataSourceRunErrorWrapper;
use crate::search::slash_command_menu::StaticCommand;
use crate::search::slash_command_menu::static_commands::Availability;
use crate::search::slash_command_menu::static_commands::commands::{self, COMMAND_REGISTRY};
use crate::settings::{
    InputSettings, InputSettingsChangedEvent, PrivacySettings, PrivacySettingsChangedEvent,
};
use crate::terminal::input::slash_commands::AcceptSlashCommandOrSavedPrompt;
use crate::terminal::model::session::active_session::ActiveSession;

pub struct GuiDataSourceArgs {
    pub active_session: ModelHandle<ActiveSession>,
    pub terminal_view_id: EntityId,
}

pub struct GuiSlashCommandDataSource {
    state: SlashCommandDataSourceState,
    is_cloud_mode_v2: bool,
}

impl GuiSlashCommandDataSource {
    pub fn new(args: GuiDataSourceArgs, ctx: &mut ModelContext<Self>) -> Self {
        Self::build(args, false, ctx)
    }

    pub fn for_cloud_mode_v2(args: GuiDataSourceArgs, ctx: &mut ModelContext<Self>) -> Self {
        Self::build(args, true, ctx)
    }

    fn build(
        args: GuiDataSourceArgs,
        is_cloud_mode_v2: bool,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        let GuiDataSourceArgs {
            active_session,
            terminal_view_id,
        } = args;

        subscribe_to_shared_dependencies(
            &active_session,
            terminal_view_id,
            Self::recompute_active_commands,
            ctx,
        );
        // Preserve the existing GUI subscriptions whose settings affect GUI-only command gates.
        ctx.subscribe_to_model(&PrivacySettings::handle(ctx), |me, _, event, ctx| {
            if matches!(
                event,
                PrivacySettingsChangedEvent::UpdateIsCloudConversationStorageEnabled { .. }
            ) {
                me.recompute_active_commands(ctx);
            }
        });
        ctx.subscribe_to_model(&InputSettings::handle(ctx), |me, _, event, ctx| {
            if matches!(
                event,
                InputSettingsChangedEvent::EnableSlashCommandsInTerminal { .. }
            ) {
                me.recompute_active_commands(ctx);
            }
        });

        let mut me = Self {
            state: SlashCommandDataSourceState::new(active_session, terminal_view_id),
            is_cloud_mode_v2,
        };
        me.recompute_active_commands(ctx);
        me
    }

    pub(super) fn is_cloud_mode_v2(&self) -> bool {
        self.is_cloud_mode_v2
    }

    pub fn is_agent_view_active(&self, _ctx: &AppContext) -> bool {
        // LOCAL FORK: there is no agent view to be in anymore.
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

    pub(crate) fn command_is_active(&self, command: &StaticCommand, ctx: &AppContext) -> bool {
        let availability = self.availability(ctx);
        let gates = self.common_command_gates(ctx);
        self.command_passes_common_gates(command, availability, &gates)
            && self.command_passes_gui_gates(
                command,
                availability,
                #[cfg(not(target_family = "wasm"))]
                ctx,
            )
    }

    fn recompute_active_commands(&mut self, ctx: &mut ModelContext<Self>) {
        let availability = self.availability(ctx);
        let gates = self.common_command_gates(ctx);
        let commands = HashMap::from_iter(
            COMMAND_REGISTRY
                .all_commands_by_id()
                .filter(|(_, command)| {
                    self.command_passes_common_gates(command, availability, &gates)
                        && self.command_passes_gui_gates(
                            command,
                            availability,
                            #[cfg(not(target_family = "wasm"))]
                            ctx,
                        )
                })
                .map(|(id, command)| (id, command.clone())),
        );
        if self.replace_active_commands(commands) {
            ctx.emit(UpdatedActiveCommands);
        }
    }

    fn availability(&self, ctx: &AppContext) -> Availability {
        let is_agent_view_active = self.is_agent_view_active(ctx);
        let mut availability =
            self.base_availability(ctx) | Self::view_availability(is_agent_view_active);

        if self.has_active_conversation(is_agent_view_active, ctx) {
            availability |= Availability::ACTIVE_CONVERSATION;
        }

        if self.is_cloud_mode_v2 && FeatureFlag::CloudModeInputV2.is_enabled() {
            availability |= Availability::CLOUD_MODE_V2_COMPOSER;
        }

        if self.is_cloud_mode(ctx) {
            availability |= Availability::CLOUD_AGENT;
        } else {
            availability |= Availability::NOT_CLOUD_AGENT;
        }

        availability
    }

    /// View-related availability bits for the GUI's legacy terminal-view and agent-view
    /// modalities. When the AgentView feature flag is disabled, both bits are set so either
    /// requirement is satisfied.
    fn view_availability(is_agent_view_active: bool) -> Availability {
        if !FeatureFlag::AgentView.is_enabled() {
            Availability::AGENT_VIEW | Availability::TERMINAL_VIEW
        } else if is_agent_view_active {
            Availability::AGENT_VIEW
        } else {
            Availability::TERMINAL_VIEW
        }
    }

    fn command_passes_gui_gates(
        &self,
        command: &StaticCommand,
        availability: Availability,
        #[cfg(not(target_family = "wasm"))] _ctx: &AppContext,
    ) -> bool {
        if command.name == commands::FORK.name
            && availability.contains(Availability::CLOUD_MODE_V2_COMPOSER)
        {
            return false;
        }
        // /continue-locally only applies to cloud Oz conversations.
        //
        // LOCAL FORK: `active_conversation_is_cloud_oz` went with the agent and there
        // are no conversations left, so the command is always filtered out rather than
        // being surfaced in the slash menu as a no-op.
        #[cfg(not(target_family = "wasm"))]
        if command.name == commands::CONTINUE_LOCALLY.name {
            return false;
        }
        true
    }

    fn is_cloud_mode(&self, _ctx: &AppContext) -> bool {
        // LOCAL FORK: without the ambient agent view model, only the cloud mode v2
        // composer can be a cloud pane.
        self.is_cloud_mode_v2
    }
}

impl SyncDataSource for GuiSlashCommandDataSource {
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
        // Skills invoke locally, so they're hidden on any cloud pane (live viewer,
        // disconnected follow-up, or read-only tombstone).
        if !self.is_cloud_mode(app) {
            results.extend(self.match_skills(&query_text, app));
        }

        Ok(results
            .into_iter()
            .map(|item: InlineItem| item.with_compact_layout(self.is_cloud_mode_v2).into())
            .collect())
    }
}

impl SlashCommandDataSource for GuiSlashCommandDataSource {
    fn state(&self) -> &SlashCommandDataSourceState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut SlashCommandDataSourceState {
        &mut self.state
    }
}
impl Entity for GuiSlashCommandDataSource {
    type Event = UpdatedActiveCommands;
}
