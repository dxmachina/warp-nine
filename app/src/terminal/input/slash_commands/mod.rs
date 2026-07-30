mod cloud_mode_v2_view;
mod data_source;
mod mixer;
mod search_item;
pub(super) mod view;

#[cfg(feature = "local_fs")]
use std::path::PathBuf;

pub use cloud_mode_v2_view::{CloudModeV2SlashCommandView, Section as CloudModeV2Section};
pub use data_source::*;
pub use mixer::{SlashCommandMixer, build_slash_command_mixer, slash_command_query};
pub use view::{CloseReason, InlineSlashCommandView, SlashCommandsEvent};
use warp_core::features::FeatureFlag;
use warp_core::send_telemetry_from_ctx;
use warp_core::ui::theme::AnsiColorIdentifier;
#[cfg(feature = "local_fs")]
use warp_util::path::{CleanPathResult, LineAndColumnArg};
use warpui::{AppContext, SingletonEntity, ViewContext};

use crate::TelemetryEvent;
use crate::cloud_object::model::persistence::CloudModel;
use crate::code_review::telemetry_event::CodeReviewPaneEntrypoint;
use crate::search::slash_command_menu::static_commands::commands::COMMAND_REGISTRY;
use crate::search::slash_command_menu::static_commands::{Availability, SlashCommandKind};
use crate::search::slash_command_menu::{SlashCommandId, StaticCommand};
use crate::server::ids::SyncId;
use crate::server::telemetry::{AgentModeAutoDetectionSettingOrigin, SlashCommandAcceptedDetails};
use crate::settings::AISettings;
use crate::skills::SkillReference;
use crate::tab::SelectedTabColor;
use crate::terminal::input::decorations::InputBackgroundJobOptions;
use crate::terminal::input::inline_menu::{InlineMenuAction, InlineMenuType};
use crate::terminal::input::slash_command_model::{
    SlashCommandEntryState, UpdatedSlashCommandModel,
};
use crate::terminal::input::{CompletionsTrigger, Event, Input, InputSuggestionsMode};
#[cfg(feature = "local_fs")]
use crate::terminal::model::session::Session;
use crate::terminal::view::TerminalAction;
use crate::ui_components::color_dot;
use crate::view_components::DismissibleToast;
use crate::workflows::command_parser::compute_workflow_display_data;
use crate::workflows::{WorkflowSelectionSource, WorkflowSource, WorkflowType};
use crate::workspace::{ToastStack, WorkspaceAction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptSlashCommandOrSavedPrompt {
    SlashCommand {
        id: SlashCommandId,
    },
    SavedPrompt {
        id: SyncId,
    },
    /// A skill selected from browse or search. Contains name (for display/insertion) and path/bundled_skill_id (for execution).
    Skill {
        reference: SkillReference,
        name: String,
    },
}
impl InlineMenuAction for AcceptSlashCommandOrSavedPrompt {
    const MENU_TYPE: InlineMenuType = InlineMenuType::SlashCommands;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommandSelectionBehavior {
    InsertCommandText(String),
    Execute,
}

/// Shared menu-selection policy for static slash commands.
///
/// GUI and TUI both first decide whether accepting a menu row should insert the
/// slash command text for further argument entry, or execute the command
/// immediately. Surface-specific execution remains in `Input::execute_slash_command`
/// for GUI and in `TuiTerminalSessionView::execute_tui_slash_command` for TUI.
pub fn slash_command_selection_behavior(command: &StaticCommand) -> SlashCommandSelectionBehavior {
    if command
        .argument
        .as_ref()
        .is_some_and(|argument| !argument.should_execute_on_selection)
    {
        SlashCommandSelectionBehavior::InsertCommandText(format!("{} ", command.name))
    } else {
        SlashCommandSelectionBehavior::Execute
    }
}

/// Whether an already-open slash command menu should close after the input becomes an exact
/// static-command or skill match.
///
/// This preserves the GUI's existing behavior: exact input stays visible while multiple prior
/// results remain, but a unique match or the start of argument entry closes the menu.
pub fn should_close_slash_command_menu_for_exact_match(
    result_count: usize,
    argument_started: bool,
) -> bool {
    result_count < 2 || argument_started
}

/// Records a static slash command accepted from either the GUI or TUI surface.
pub fn record_static_slash_command_accepted(
    command_name: &str,
    is_in_agent_view: bool,
    ctx: &mut AppContext,
) {
    send_telemetry_from_ctx!(
        TelemetryEvent::SlashCommandAccepted {
            command_details: SlashCommandAcceptedDetails::StaticCommand {
                command_name: command_name.to_owned(),
            },
            is_in_agent_view,
        },
        ctx
    );
}

/// Records an input auto-detection setting toggle triggered from a TUI slash
/// command (`/natural-language-detection`).
///
/// Mirrors the `SettingsPage` and `Banner` origins used by the GUI toggle paths,
/// but reports the toggle as originating from a TUI slash command.
pub fn record_autodetection_toggle_from_slash_command(
    is_autodetection_enabled: bool,
    ctx: &mut AppContext,
) {
    send_telemetry_from_ctx!(
        TelemetryEvent::AgentModeToggleAutoDetectionSetting {
            is_autodetection_enabled,
            origin: AgentModeAutoDetectionSettingOrigin::SlashCommand,
        },
        ctx
    );
}

/// Records a saved prompt accepted from either the GUI or TUI slash menu.
pub fn record_saved_prompt_accepted(is_in_agent_view: bool, ctx: &mut AppContext) {
    send_telemetry_from_ctx!(
        TelemetryEvent::SlashCommandAccepted {
            command_details: SlashCommandAcceptedDetails::SavedPrompt,
            is_in_agent_view,
        },
        ctx
    );
}

pub fn saved_prompt_text_for_id(id: &SyncId, ctx: &AppContext) -> Option<String> {
    let workflow = CloudModel::as_ref(ctx).get_workflow(id)?;
    workflow.model().data.is_agent_mode_workflow().then(|| {
        compute_workflow_display_data(&workflow.model().data).command_with_replaced_arguments
    })
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SlashCommandTrigger {
    Input { cmd_or_ctrl_enter: bool },
    Keybinding,
}

impl SlashCommandTrigger {
    fn cmd_or_ctrl_enter() -> Self {
        Self::Input {
            cmd_or_ctrl_enter: true,
        }
    }

    pub fn input() -> Self {
        Self::Input {
            cmd_or_ctrl_enter: false,
        }
    }

    pub(super) fn keybinding() -> Self {
        Self::Keybinding
    }

    pub fn is_keybinding(&self) -> bool {
        matches!(self, Self::Keybinding)
    }

    fn is_cmd_or_ctrl_enter(&self) -> bool {
        matches!(
            self,
            Self::Input {
                cmd_or_ctrl_enter: true
            }
        )
    }
}

#[cfg(feature = "local_fs")]
fn open_file_command_path(
    session: &Session,
    current_dir: &str,
    raw_arg: &str,
) -> (PathBuf, Option<LineAndColumnArg>) {
    let parsed_path = CleanPathResult::with_line_and_column_number(raw_arg.trim());
    // The argument may contain shell-escaped characters (e.g. `\ ` for spaces) from auto-suggest.
    // Unescape them so the path matches the actual filesystem entry.
    let unescaped_path = session.shell_family().unescape(&parsed_path.path);
    // Expand `~` to the user's home directory.
    let expanded_path = shellexpand::tilde(&unescaped_path);

    let shell_path = session
        .convert_directory_to_typed_path_buf(current_dir.to_owned())
        .join(session.convert_directory_to_typed_path_buf(expanded_path.into_owned()))
        .normalize();
    let file_path = session
        .maybe_convert_to_native_path(&shell_path.to_path())
        .unwrap_or_else(|err| {
            log::warn!("unable to convert /open-file path to native path: {err:?}");
            PathBuf::from(shell_path.to_string_lossy().into_owned())
        });

    (file_path, parsed_path.line_and_column_num)
}

impl Input {
    fn is_slash_command_available(&self, command: &StaticCommand, ctx: &AppContext) -> bool {
        let slash_command_data_source = if self.is_cloud_mode_input_v2_composing(ctx) {
            let Some(data_source) = self.cloud_mode_composer_slash_command_data_source.as_ref()
            else {
                return false;
            };
            data_source
        } else {
            &self.slash_command_data_source
        };
        slash_command_data_source
            .as_ref(ctx)
            .command_is_active(command, ctx)
    }

    pub(super) fn select_slash_command(
        &mut self,
        command: &StaticCommand,
        trigger: SlashCommandTrigger,
        ctx: &mut ViewContext<Self>,
    ) {
        if !self.is_slash_command_available(command, ctx) {
            return;
        }
        match slash_command_selection_behavior(command) {
            SlashCommandSelectionBehavior::Execute => {
                // TODO (zachbai): this is a hack for Oz launch. Caller
                // should probably be invoking `execute_slash_command` in this case.
                let argument = if command
                    .argument
                    .as_ref()
                    .is_some_and(|arg| arg.should_execute_on_selection)
                    && !self.suggestions_mode_model.as_ref(ctx).is_slash_commands()
                {
                    let trimmed = self.buffer_text(ctx).trim().to_owned();
                    (!trimmed.is_empty()).then_some(trimmed)
                } else {
                    None
                };
                self.execute_slash_command(
                    command,
                    argument.as_ref(),
                    trigger,
                    /*is_queued_prompt*/ false,
                    ctx,
                );
            }
            SlashCommandSelectionBehavior::InsertCommandText(text) => {
                self.editor.update(ctx, |editor, ctx| {
                    editor.set_buffer_text(&text, ctx);
                });
            }
        }
    }

    pub(super) fn close_slash_commands_menu(&mut self, ctx: &mut ViewContext<Self>) {
        self.suggestions_mode_model.update(ctx, |model, ctx| {
            model.set_mode(InputSuggestionsMode::Closed, ctx);
        });
        ctx.notify();
    }

    pub(super) fn handle_slash_command_model_event(
        &mut self,
        event: &UpdatedSlashCommandModel,
        ctx: &mut ViewContext<Self>,
    ) {
        // Refresh decorations if the slash command detection state changed, since
        // detected commands affect syntax highlighting.
        let new_state = self.slash_command_model.as_ref(ctx).state();
        if event.old_state.is_detected_command() != new_state.is_detected_command() {
            let _ = self
                .debounce_input_background_tx
                .try_send(InputBackgroundJobOptions::default().with_command_decoration());
        }

        match self.slash_command_model.as_ref(ctx).state().clone() {
            SlashCommandEntryState::None => {
                if self.suggestions_mode_model.as_ref(ctx).is_slash_commands() {
                    self.close_slash_commands_menu(ctx);
                }
            }
            SlashCommandEntryState::Composing { .. } => {
                if self.suggestions_mode_model.as_ref(ctx).is_closed() {
                    self.open_slash_commands_menu(ctx);
                } else if !self.suggestions_mode_model.as_ref(ctx).is_slash_commands() {
                    self.slash_command_model.update(ctx, |model, ctx| {
                        model.disable(ctx);
                    });
                }
            }
            SlashCommandEntryState::SlashCommand(detected_command) => {
                // If there is only one result (or zero, but that should be impossible if there is
                // a valid command in the input) OR if the user has started typing arguments, hide
                // the menu.
                if self.suggestions_mode_model.as_ref(ctx).is_slash_commands()
                    && should_close_slash_command_menu_for_exact_match(
                        self.inline_slash_commands_view
                            .as_ref(ctx)
                            .result_count(ctx),
                        detected_command.argument.is_some(),
                    )
                {
                    self.close_slash_commands_menu(ctx);
                }

                // LOCAL FORK: entering AI mode for a slash command went with the agent.
                if detected_command.command.kind == SlashCommandKind::Edit
                    && detected_command
                        .argument
                        .as_ref()
                        .is_some_and(|argument| argument.is_empty())
                    && self.suggestions_mode_model.as_ref(ctx).is_closed()
                {
                    self.open_completion_suggestions(CompletionsTrigger::SlashCommandAutoOpen, ctx);
                }
            }
            SlashCommandEntryState::SkillCommand(detected_skill) => {
                // Hide the menu once the user has started typing the prompt
                if self.suggestions_mode_model.as_ref(ctx).is_slash_commands()
                    && should_close_slash_command_menu_for_exact_match(
                        self.inline_slash_commands_view
                            .as_ref(ctx)
                            .result_count(ctx),
                        detected_skill.argument.is_some(),
                    )
                {
                    self.close_slash_commands_menu(ctx);
                }

                // LOCAL FORK: skill commands used to force the input into AI mode.
            }
        }
    }

    pub(crate) fn handle_slash_commands_menu_event(
        &mut self,
        event: &SlashCommandsEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            SlashCommandsEvent::Close(reason) => {
                if reason.is_manual_dismissal() {
                    self.slash_command_model.update(ctx, |model, ctx| {
                        model.disable(ctx);
                    });
                }

                self.suggestions_mode_model.update(ctx, |model, ctx| {
                    model.set_mode(InputSuggestionsMode::Closed, ctx);
                });
                ctx.notify();
            }
            SlashCommandsEvent::SelectedSavedPrompt { id } => {
                let Some(workflow) = CloudModel::as_ref(ctx).get_workflow(id).cloned() else {
                    log::warn!("Tried to execute workflow for id {id:?} but it does not exist");
                    return;
                };
                // LOCAL FORK: there is no agent view to be in.
                record_saved_prompt_accepted(/*is_in_agent_view*/ false, ctx);

                self.show_workflows_info_box_on_workflow_selection(
                    WorkflowType::Cloud(Box::new(workflow)),
                    WorkflowSource::WarpAI,
                    WorkflowSelectionSource::SlashMenu,
                    None,
                    ctx,
                );
            }
            SlashCommandsEvent::SelectedStaticCommand {
                id,
                cmd_or_ctrl_enter,
            } => {
                let Some(command) = COMMAND_REGISTRY.get_command(id) else {
                    return;
                };
                self.select_slash_command(
                    command,
                    SlashCommandTrigger::Input {
                        cmd_or_ctrl_enter: *cmd_or_ctrl_enter,
                    },
                    ctx,
                );
            }
            SlashCommandsEvent::SelectedSkill { name, reference: _ } => {
                // Insert /{skill-name} into the buffer
                self.editor.update(ctx, |editor, ctx| {
                    editor.set_buffer_text(format!("/{name} ").as_str(), ctx);
                });
                self.close_slash_commands_menu(ctx);
            }
        }
    }

    /// Executes the given `command` with `argument`, if any.
    ///
    /// When `is_queued_prompt` is true, this is the first send of a previously queued prompt:
    /// the input buffer is left alone so the user doesn't lose anything they've typed while
    /// the agent was busy.
    ///
    /// Returns `true` if execution was 'handled' (whether or not it resulted in success or failure).
    // LOCAL FORK: the queued-prompt conversation / query ids came out with the agent's
    // prompt queue.
    pub(super) fn execute_slash_command(
        &mut self,
        command: &StaticCommand,
        argument: Option<&String>,
        trigger: SlashCommandTrigger,
        is_queued_prompt: bool,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        fn show_error_toast(message: String, ctx: &mut ViewContext<Input>) {
            let window_id = ctx.window_id();
            ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                toast_stack.add_ephemeral_toast(DismissibleToast::error(message), window_id, ctx);
            });
        }

        // Safety net: commands whose availability requires AI should not execute when AI is
        // globally disabled. They're normally filtered out of the slash command menu, but this
        // protects keybinding-triggered execution where a bound key may still address the command.
        if command.availability.contains(Availability::AI_ENABLED)
            && !AISettings::as_ref(ctx).is_any_ai_enabled(ctx)
        {
            show_error_toast(format!("{} requires AI to be enabled", command.name), ctx);
            return true;
        }

        // Handle the slash command action based on its kind
        match command.kind {
            SlashCommandKind::AddMcp => {
                ctx.dispatch_typed_action(&TerminalAction::OpenAddMCPPane);
            }
            SlashCommandKind::AddPrompt => {
                ctx.dispatch_typed_action(&TerminalAction::OpenAddPromptPane);
            }
            SlashCommandKind::AddRule => {
                ctx.dispatch_typed_action(&TerminalAction::OpenAddRulePane);
            }
            // LOCAL FORK: /agent, /new and /cloud-agent all opened an agent view.
            SlashCommandKind::Agent | SlashCommandKind::New | SlashCommandKind::CloudAgent => {
                return false;
            }
            SlashCommandKind::CreateDockerSandbox => {
                ctx.emit(Event::CreateDockerSandbox);
            }
            // LOCAL FORK: /conversations browsed agent conversations.
            SlashCommandKind::Conversations => return false,
            SlashCommandKind::RenameTab => {
                let Some(name) = argument
                    .map(|name| name.trim())
                    .filter(|name| !name.is_empty())
                else {
                    show_error_toast(
                        "Please provide a tab name after /rename-tab".to_owned(),
                        ctx,
                    );
                    return true;
                };

                ctx.dispatch_typed_action(&WorkspaceAction::SetActiveTabName(name.to_owned()));
            }
            // LOCAL FORK: /rename-conversation renamed an agent conversation.
            SlashCommandKind::RenameConversation => return false,
            SlashCommandKind::SetTabColor => {
                let supported_options = || {
                    color_dot::TAB_COLOR_OPTIONS
                        .iter()
                        .map(|c| c.to_string().to_ascii_lowercase())
                        .chain(std::iter::once("none".to_owned()))
                        .collect::<Vec<_>>()
                        .join(", ")
                };

                let Some(arg) = argument
                    .map(|name| name.trim())
                    .filter(|name| !name.is_empty())
                else {
                    show_error_toast(
                        format!(
                            "Please provide a color after /set-tab-color ({})",
                            supported_options()
                        ),
                        ctx,
                    );
                    return true;
                };

                let color = if arg.eq_ignore_ascii_case("none") {
                    SelectedTabColor::Cleared
                } else {
                    let parsed = arg
                        .parse::<AnsiColorIdentifier>()
                        .ok()
                        .filter(|c| color_dot::TAB_COLOR_OPTIONS.contains(c));
                    match parsed {
                        Some(c) => SelectedTabColor::Color(c),
                        None => {
                            show_error_toast(
                                format!(
                                    "Unknown tab color '{arg}'. Use one of: {}.",
                                    supported_options()
                                ),
                                ctx,
                            );
                            return true;
                        }
                    }
                };

                ctx.dispatch_typed_action(&WorkspaceAction::SetActiveTabColor(color));
            }
            SlashCommandKind::CreateEnvironment => {
                // If the user included args after the slash command, treat them as repo paths/URLs.
                let repos = argument
                    .map(|arg| {
                        arg.split_whitespace()
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                ctx.emit(Event::TriggerEnvironmentSetup { repos });
            }
            // LOCAL FORK: /create-new-project sent the description to the agent.
            SlashCommandKind::CreateNewProject => return false,
            SlashCommandKind::Edit => {
                #[cfg(feature = "local_fs")]
                match argument {
                    Some(args) if !args.is_empty() => {
                        let Some(session_id) = self.active_block_session_id() else {
                            return false;
                        };

                        let Some(session) = self.sessions.as_ref(ctx).get(session_id) else {
                            return false;
                        };

                        if !session.is_local() {
                            let window_id = ctx.window_id();
                            ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                                toast_stack.add_ephemeral_toast(
                                    DismissibleToast::error(
                                        "The /open-file command is only available for local sessions"
                                            .to_owned(),
                                    ),
                                    window_id,
                                    ctx,
                                );
                            });
                            return false;
                        }

                        let current_dir = self
                            .active_block_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.current_working_directory())
                            .map(str::to_owned);

                        let Some(current_dir) = current_dir else {
                            return false;
                        };

                        let (file_path, line_col) =
                            open_file_command_path(&session, &current_dir, args);

                        match std::fs::metadata(&file_path) {
                            Ok(metadata) if metadata.is_file() => {
                                use crate::util::file::external_editor;

                                ctx.dispatch_typed_action(&TerminalAction::OpenCodeInWarp {
                                    path: file_path,
                                    layout: external_editor::settings::EditorLayout::SplitPane,
                                    line_col,
                                });
                            }
                            Ok(_) => {
                                show_error_toast(
                                    "The /open-file command only works for files, not directories"
                                        .to_owned(),
                                    ctx,
                                );
                                return true;
                            }
                            Err(_) => {
                                show_error_toast(
                                    format!("File not found: {}", file_path.display()),
                                    ctx,
                                );
                                return true;
                            }
                        }
                    }
                    _ => {
                        use crate::server::telemetry::PaletteSource;

                        ctx.emit(Event::OpenFilesPalette {
                            source: PaletteSource::Keybinding,
                        });
                    }
                }
                #[cfg(not(feature = "local_fs"))]
                {
                    show_error_toast(
                        "The /open-file command is not supported in this build".to_owned(),
                        ctx,
                    );
                    return true;
                }
            }
            // LOCAL FORK: /export-to-clipboard and /export-to-file exported an agent
            // conversation.
            SlashCommandKind::ExportToClipboard | SlashCommandKind::ExportToFile => return false,
            SlashCommandKind::Index => {
                ctx.dispatch_typed_action(&TerminalAction::IndexProjectSpeedbump);
            }
            SlashCommandKind::Init => {
                ctx.dispatch_typed_action(&TerminalAction::InitProject);
            }
            SlashCommandKind::Changelog => {
                if !FeatureFlag::Changelog.is_enabled() {
                    return false;
                }
                ctx.dispatch_typed_action(&WorkspaceAction::ViewLatestChangelog);
            }
            SlashCommandKind::Feedback => {
                ctx.dispatch_typed_action(&WorkspaceAction::SendFeedback);
            }
            SlashCommandKind::OpenCodeReview => {
                ctx.dispatch_typed_action(&TerminalAction::ToggleCodeReviewPane {
                    entrypoint: CodeReviewPaneEntrypoint::SlashCommand,
                });
            }
            SlashCommandKind::OpenMcpServers | SlashCommandKind::Mcp => {
                ctx.dispatch_typed_action(&TerminalAction::OpenViewMCPPane);
            }
            SlashCommandKind::OpenSettingsFile => {
                if !FeatureFlag::SettingsFile.is_enabled() || !cfg!(feature = "local_fs") {
                    return false;
                }
                ctx.dispatch_typed_action(&WorkspaceAction::OpenSettingsFile);
            }
            SlashCommandKind::OpenProjectRules => {
                ctx.dispatch_typed_action(&TerminalAction::OpenProjectRulesPane);
            }
            SlashCommandKind::OpenRules => {
                ctx.dispatch_typed_action(&TerminalAction::OpenRulesPane);
            }
            // LOCAL FORK: the skill selectors, the cloud-mode host / harness / environment
            // selectors, the agent model and execution-profile selectors and the saved
            // prompts menu all came out with the agent.
            SlashCommandKind::EditSkill
            | SlashCommandKind::InvokeSkill
            | SlashCommandKind::Host
            | SlashCommandKind::Harness
            | SlashCommandKind::Environment
            | SlashCommandKind::Model
            | SlashCommandKind::Profile
            | SlashCommandKind::Prompts => return false,
            SlashCommandKind::Rewind => {
                self.open_rewind_menu(ctx);
            }
            SlashCommandKind::Usage => {
                ctx.dispatch_typed_action(&TerminalAction::OpenBillingAndUsagePane);
            }
            SlashCommandKind::RemoteControl => {
                if !FeatureFlag::CreatingSharedSessions.is_enabled()
                    || !FeatureFlag::HOARemoteControl.is_enabled()
                {
                    return false;
                }
                if self
                    .model
                    .lock()
                    .shared_session_status()
                    .is_sharer_or_viewer()
                {
                    show_error_toast("Session is already being shared".to_owned(), ctx);
                    return true;
                }
                ctx.emit(Event::StartRemoteControl);
            }
            // LOCAL FORK: /cost, /handoff, /fork, /fork-from, /continue-locally,
            // /fork-and-compact, /compact-and and /queue all acted on an agent
            // conversation and came out with the agent.
            SlashCommandKind::Cost
            | SlashCommandKind::MoveToCloud
            | SlashCommandKind::Fork
            | SlashCommandKind::ForkFrom
            | SlashCommandKind::ForkAndCompact
            | SlashCommandKind::CompactAnd
            | SlashCommandKind::Queue
            | SlashCommandKind::ContinueLocally => return false,
            SlashCommandKind::OpenRepo => {
                if !FeatureFlag::InlineRepoMenu.is_enabled() {
                    return false;
                }
                self.open_repos_menu(ctx);
            }
            SlashCommandKind::Compact | SlashCommandKind::Plan | SlashCommandKind::Orchestrate => {
                // These slash commands just send AI requests with the slash command text as a
                // prefix, and special handling is done downstream as an implementation detail
                // of handling user queries with specific slash command prefixes.
                return false;
            }
            SlashCommandKind::AutoApprove
            | SlashCommandKind::Statusline
            | SlashCommandKind::AddApiKey
            | SlashCommandKind::ClearApiKey
            | SlashCommandKind::ViewLogs
            | SlashCommandKind::Voice
            | SlashCommandKind::NaturalLanguageDetection
            | SlashCommandKind::Theme
            | SlashCommandKind::Exit
            | SlashCommandKind::Logout
            | SlashCommandKind::Clear
            | SlashCommandKind::Status => {
                debug_assert!(
                    false,
                    "Attempted to execute TUI-only slash command in the GUI: {}",
                    command.name
                );
                return false;
            }
        }

        // Leave the buffer alone when re-sending a queued prompt (the user may have typed
        // new input while the agent was busy).
        if !is_queued_prompt {
            self.editor.update(ctx, |editor, ctx| {
                editor.clear_buffer(ctx);
            });
        }

        // LOCAL FORK: auto-entering the agent view for `auto_enter_ai_mode` commands went
        // with the agent, and there is no agent view to report being in.
        record_static_slash_command_accepted(command.name, /*is_in_agent_view*/ false, ctx);
        true
    }

    /// Handles cmd+enter (Mac) / ctrl+enter (Linux/Windows) for slash commands.
    ///
    /// Returns `true` if the keypress was handled.
    pub(super) fn maybe_handle_cmd_or_ctrl_shift_enter_for_slash_command(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        // If slash command menu is open, accept the selected item with cmd_or_ctrl_enter=true.
        if matches!(
            self.suggestions_mode_model.as_ref(ctx).mode(),
            InputSuggestionsMode::SlashCommands
        ) {
            if self.is_cloud_mode_input_v2_composing(ctx) {
                if let Some(view) = self.cloud_mode_v2_slash_commands_view.clone() {
                    view.update(ctx, |view, ctx| {
                        view.accept_selected_item(true, ctx);
                    });
                }
            } else {
                self.inline_slash_commands_view.update(ctx, |view, ctx| {
                    view.accept_selected_item(true, ctx);
                });
            }
            return true;
        }

        // If no menu but slash command detected in buffer, execute with cmd_or_ctrl_enter=true
        match self.slash_command_model.as_ref(ctx).state() {
            SlashCommandEntryState::SlashCommand(detected_command) => {
                let command = detected_command.command.clone();
                let argument = detected_command.argument.clone();
                if !self.is_slash_command_available(&command, ctx) {
                    return false;
                }
                self.execute_slash_command(
                    &command,
                    argument.as_ref(),
                    SlashCommandTrigger::cmd_or_ctrl_enter(),
                    /*is_queued_prompt*/ false,
                    ctx,
                )
            }
            // LOCAL FORK: executing a skill command sent it to the agent.
            SlashCommandEntryState::SkillCommand(_)
            | SlashCommandEntryState::None
            | SlashCommandEntryState::Composing { .. } => false,
        }
    }

    fn apply_v2_slash_section_filter(
        &mut self,
        section: CloudModeV2Section,
        ctx: &mut ViewContext<Self>,
    ) {
        self.editor.update(ctx, |editor, ctx| {
            editor.set_buffer_text("/", ctx);
        });
        if let Some(view) = self.cloud_mode_v2_slash_commands_view.clone() {
            view.update(ctx, |v, ctx| {
                v.set_section_filter(Some(section), ctx);
            });
        }
    }

    pub(super) fn maybe_clear_v2_slash_section_filter(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        if !self.is_cloud_mode_input_v2_composing(ctx) {
            return false;
        }
        let Some(view) = self.cloud_mode_v2_slash_commands_view.clone() else {
            return false;
        };
        let has_filter = view.as_ref(ctx).has_section_filter();
        if !has_filter {
            return false;
        }
        view.update(ctx, |v, ctx| {
            v.set_section_filter(None, ctx);
        });
        true
    }

    /// Executes a slash command on `enter` keypress.
    ///
    /// If the slash command menu is open, then "accepts" the slash command:
    ///   * If the slash command does not take arguments, executes it
    ///   * If the slash command does take arguments, inserts it into the input.
    ///
    /// If the slash command menu is not open, then "executes" the slash command in the input, if
    /// there is one.
    ///
    /// Returns `true` if the enter keypress was 'handled', else upstream enter keypress handling
    /// logic should continue.
    pub(super) fn maybe_handle_enter_for_slash_command(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        if matches!(
            self.suggestions_mode_model.as_ref(ctx).mode(),
            InputSuggestionsMode::SlashCommands
        ) {
            if self.is_cloud_mode_input_v2_composing(ctx) {
                if let Some(view) = self.cloud_mode_v2_slash_commands_view.clone() {
                    view.update(ctx, |view, ctx| {
                        view.accept_selected_item(false, ctx);
                    });
                }
            } else {
                self.inline_slash_commands_view.update(ctx, |view, ctx| {
                    view.accept_selected_item(false, ctx);
                });
            }
            return true;
        }

        match self.slash_command_model.as_ref(ctx).state() {
            SlashCommandEntryState::SlashCommand(detected_command) => {
                let command = detected_command.command.clone();
                let argument = detected_command.argument.clone();
                if !self.is_slash_command_available(&command, ctx) {
                    return false;
                }
                self.execute_slash_command(
                    &command,
                    argument.as_ref(),
                    SlashCommandTrigger::input(),
                    /*is_queued_prompt*/ false,
                    ctx,
                )
            }
            // LOCAL FORK: executing a skill command sent it to the agent.
            SlashCommandEntryState::SkillCommand(_)
            | SlashCommandEntryState::None
            | SlashCommandEntryState::Composing { .. } => false,
        }
    }
}

/// Whether executing the static slash `command` submits its text to the conversation as an AI
/// prompt (handled downstream like a normal user query) rather than performing an immediate
/// local action.
///
/// This is the single source of truth for the "reiterated as a prompt vs handled immediately"
/// distinction: only `/compact`, `/plan`, and `/orchestrate` are sent as prompts (mirroring the
/// `command_that_just_sends_ai_request_with_prefix` arm in [`Input::execute_slash_command`]).
/// Every other slash command emits an immediate action (forking, switching model, opening a
/// menu, etc.), so callers gating prompt queuing or shared-session forwarding should treat those
/// as "run now".
pub fn slash_command_is_submitted_as_prompt(command: &StaticCommand) -> bool {
    matches!(
        command.kind,
        SlashCommandKind::Compact | SlashCommandKind::Plan | SlashCommandKind::Orchestrate
    )
}

// LOCAL FORK: ForkButtonAction / fork_button_action / conversation_is_cloud_oz_for_slash_command
// removed with the agent; they described the `/fork` vs `/continue-locally` choice for an
// agent conversation.

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
