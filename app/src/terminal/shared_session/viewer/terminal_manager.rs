use std::any::Any;
use std::sync::Arc;

use async_broadcast::InactiveReceiver;
use parking_lot::FairMutex;
use pathfinder_geometry::vector::Vector2F;
// LOCAL FORK: `CLIAgentSessionState`, `SelectedAgentModel` and
// `sharer::SessionSourceType` were only used by the agent-state mirroring below.
use session_sharing_protocol::common::{
    ActivePrompt, AddGuestsResponse, CommandExecutionFailureReason, LinkAccessLevelUpdateResponse,
    LongRunningCommandAgentInteraction, RemoveGuestResponse, SessionId,
    TeamAccessLevelUpdateResponse, UniversalDeveloperInputContextUpdate,
    UpdatePendingUserRoleResponse,
};
use session_sharing_protocol::viewer::SessionEndedReason;
use settings::Setting as _;
use warp_errors::report_error;
use warpui::{
    AppContext, ModelContext, ModelHandle, SingletonEntity, ViewContext, ViewHandle,
    WeakViewHandle, WindowId,
};

use super::event_loop::SharedSessionInitialLoadMode;
use super::network::{
    Network, NetworkEvent, agent_prompt_failure_reason_string,
    command_execution_failure_reason_string, control_action_failure_reason_string,
    session_ended_reason_string, viewer_removed_reason_string, write_to_pty_failure_reason_string,
};
// LOCAL FORK: the viewer used to mirror the sharer's agent state (conversation status,
// orchestration stream, model preferences, CLI agent sessions). All of that came out with
// the agent; only the terminal side of session viewing is kept.
use crate::context_chips::prompt_snapshot::PromptSnapshot;
use crate::context_chips::prompt_type::PromptType;
use crate::features::FeatureFlag;
use crate::network::{NetworkStatus, NetworkStatusEvent, NetworkStatusKind};
use crate::pane_group::TerminalViewResources;
use crate::pane_group::pane::DetachType;
use crate::settings::{InputModeSettings, WarpPromptSeparator};
use crate::terminal::event_listener::ChannelEventListener;
use crate::terminal::input::CommandExecutionSource;
use crate::terminal::model::ObfuscateSecrets;
use crate::terminal::model::session::Sessions;
use crate::terminal::model_events::ModelEventDispatcher;
use crate::terminal::session_settings::SessionSettings;
use crate::terminal::shared_session::SharedSessionStatus;
use crate::terminal::shared_session::manager::Manager;
use crate::terminal::shared_session::permissions_manager::SessionPermissionsManager;
// LOCAL FORK: `apply_input_mode_update`, `apply_auto_approve_agent_actions_update`
// and the `ActiveRemoteUpdate` token they took removed with the agent.
use crate::terminal::shared_session::shared_handlers::RemoteUpdateGuard;
use crate::terminal::terminal_manager::{BlockSpacing, compute_block_size, terminal_colors_list};
use crate::terminal::view::ExecuteCommandEvent;
use crate::terminal::{
    Event as TerminalViewEvent, PTY_READS_BROADCAST_CHANNEL_SIZE, TerminalModel, TerminalView,
};
use crate::view_components::ToastFlavor;

enum NetworkState {
    /// No viewer network is attached yet; deferred cloud-mode viewers start here until the
    /// follow-up shared session is created.
    Idle,
    Active(ModelHandle<Network>),
    /// Transient state while connecting a viewer network.
    Connecting,
}

struct NetworkResources {
    prompt_type: ModelHandle<PromptType>,
    channel_event_proxy: ChannelEventListener,
}

pub struct TerminalManager {
    model: Arc<FairMutex<TerminalModel>>,
    view: ViewHandle<TerminalView>,

    // We store this here just to keep it from being dropped.
    _model_events: ModelHandle<ModelEventDispatcher>,

    /// An inactive receiver for PTY reads received from the sharer over the network.
    /// We hold onto this so that the broadcast channel isn't closed prematurely.
    _inactive_pty_reads_rx: InactiveReceiver<Arc<Vec<u8>>>,

    /// The network state for the shared session viewer.
    network_state: NetworkState,
    network_resources: NetworkResources,
    current_network: Arc<FairMutex<Option<ModelHandle<Network>>>>,
    viewer_remote_update_guard: RemoteUpdateGuard,
    outbound_handlers_registered: bool,
    /// LOCAL FORK: `orchestration_viewer_model` (child discovery + status
    /// polling for an orchestrated ambient agent run) came out with the agent.
    /// This flag is retained only because it is still threaded through the
    /// public constructors that `pane_group` calls; nothing reads it now.
    #[allow(dead_code)]
    enable_orchestration_polling: bool,
}

pub struct TerminalManagerInit {
    pub(crate) manager: TerminalManager,
    pub(crate) view: ViewHandle<TerminalView>,
}

impl TerminalManager {
    // LOCAL FORK: fn send_selected_conversation_update_for_viewer_to_current_network
    // removed with the agent; it broadcast which agent conversation the viewer had
    // selected.

    fn current_network(
        current_network: &Arc<FairMutex<Option<ModelHandle<Network>>>>,
    ) -> Option<ModelHandle<Network>> {
        current_network.lock().clone()
    }

    fn update_current_network(
        current_network: &Arc<FairMutex<Option<ModelHandle<Network>>>>,
        ctx: &mut AppContext,
        update: impl FnOnce(&mut Network, &mut ModelContext<Network>),
    ) {
        let Some(network) = Self::current_network(current_network) else {
            return;
        };
        network.update(ctx, update);
    }

    fn send_input_context_update_to_current_network(
        guard: &RemoteUpdateGuard,
        model: &Arc<FairMutex<TerminalModel>>,
        current_network: &Arc<FairMutex<Option<ModelHandle<Network>>>>,
        update: UniversalDeveloperInputContextUpdate,
        ctx: &mut AppContext,
    ) {
        if !guard.should_broadcast() {
            return;
        }
        if !model.lock().shared_session_status().is_executor() {
            return;
        }

        Self::update_current_network(current_network, ctx, |network, _| {
            network.send_universal_developer_input_context_update(update);
        });
    }

    /// Handles a failed viewer command request.
    fn handle_command_execution_request_failed(
        terminal_view: &mut TerminalView,
        reason: &CommandExecutionFailureReason,
        ctx: &mut ViewContext<TerminalView>,
    ) {
        let reason_string = command_execution_failure_reason_string(reason);
        terminal_view.show_persistent_toast(reason_string, ToastFlavor::Error, ctx);
        // LOCAL FORK: `clear_queued_command_in_flight` cleared state owned by the
        // agent's queued-query model, which went with the agent.

        // On command execution request, the input is frozen and set to a loading state.
        // We only need to restore the input for errors that aren't the result of a new buffer.
        if matches!(
            reason,
            CommandExecutionFailureReason::InsufficientPermissions
        ) {
            terminal_view.input().update(ctx, |input, ctx| {
                input.on_execute_command_for_shared_session_participant_failure(ctx);
            })
        }
    }

    /// Internal constructor that creates all the models for viewing a shared session. This does not rely on the shared session existing yet.
    fn new_internal(
        resources: TerminalViewResources,
        initial_size: Vector2F,
        window_id: WindowId,
        enable_orchestration_polling: bool,
        is_ambient_agent: bool,
        ctx: &mut AppContext,
    ) -> TerminalManagerInit {
        // Create all the necessary channels we need for communication.
        let (wakeups_tx, wakeups_rx) = async_channel::unbounded();
        let (events_tx, events_rx) = async_channel::unbounded();
        let (executor_command_tx, _executor_command_rx) = async_channel::unbounded();

        // Although the viewer doesn't have a local PTY, it receives PTY bytes from the sharer
        // over the network. Those bytes are still broadcast through the ChannelEventListener,
        // so we keep an inactive listener alive for PTY recordings and other consumers.
        let (pty_reads_tx, pty_reads_rx) =
            async_broadcast::broadcast(PTY_READS_BROADCAST_CHANNEL_SIZE);
        let inactive_pty_reads_rx = pty_reads_rx.deactivate();

        let channel_event_proxy = ChannelEventListener::new(wakeups_tx, events_tx, pty_reads_tx);

        let block_spacing = BlockSpacing::for_gui(ctx);
        let show_memory_stats = block_spacing.show_memory_stats;

        // TODO: we have to figure out what prompt the viewer will see.
        // For now, just respect the viewer's settings.
        let honor_ps1 = *SessionSettings::as_ref(ctx).honor_ps1;
        let input_mode = *InputModeSettings::as_ref(ctx).input_mode.value();
        let is_inverted = input_mode.is_inverted_blocklist();

        // TODO: use the sharer's size.
        let sizes = compute_block_size(initial_size, &block_spacing, ctx);

        let model = if is_ambient_agent {
            TerminalModel::new_for_cloud_mode_shared_session_viewer(
                sizes,
                terminal_colors_list(ctx),
                channel_event_proxy.clone(),
                ctx.background_executor().clone(),
                show_memory_stats,
                honor_ps1,
                is_inverted,
                // When viewing a shared session, we don't want to apply our own
                // secret redaction rules but rather rely on the sharer obfuscating
                // the contents before reaching us.
                ObfuscateSecrets::No,
            )
        } else {
            TerminalModel::new_for_shared_session_viewer(
                sizes,
                terminal_colors_list(ctx),
                channel_event_proxy.clone(),
                ctx.background_executor().clone(),
                show_memory_stats,
                honor_ps1,
                is_inverted,
                // When viewing a shared session, we don't want to apply our own
                // secret redaction rules but rather rely on the sharer obfuscating
                // the contents before reaching us.
                ObfuscateSecrets::No,
            )
        };

        let colors = model.colors();
        let model = Arc::new(FairMutex::new(model));

        let sessions: ModelHandle<Sessions> =
            ctx.add_model(|ctx| Sessions::new(executor_command_tx, ctx));
        let cloned_model = model.clone();
        let model_events =
            ctx.add_model(|ctx| ModelEventDispatcher::new(events_rx, sessions.clone(), ctx));
        // The prompt is initially empty until we receive the update from the server.
        let prompt_type =
            ctx.add_model(|_| PromptType::new_static(vec![], false, WarpPromptSeparator::None));

        let view = ctx.add_typed_action_view(window_id, |ctx| {
            let size_info = cloned_model.lock().block_list().size().to_owned();
            TerminalView::new(
                resources,
                wakeups_rx,
                model_events.clone(),
                cloned_model,
                sessions.clone(),
                size_info,
                colors,
                None, // model_event_sender - not used for viewer
                prompt_type.clone(),
                // LOCAL FORK: the `initial_input_config` and
                // `conversation_restoration` parameters came out with the agent.
                Some(inactive_pty_reads_rx.clone()),
                is_ambient_agent,
                ctx,
            )
        });

        // LOCAL FORK: the viewer's agent view controller registration with
        // ActiveAgentViewsModel came out with the agent.

        let terminal_view = view.clone();
        let manager = Self {
            model,
            _model_events: model_events,
            view,
            _inactive_pty_reads_rx: inactive_pty_reads_rx,
            network_state: NetworkState::Idle,
            network_resources: NetworkResources {
                prompt_type,
                channel_event_proxy,
            },
            current_network: Arc::new(FairMutex::new(None)),
            viewer_remote_update_guard: RemoteUpdateGuard::new(),
            outbound_handlers_registered: false,
            enable_orchestration_polling,
        };
        TerminalManagerInit {
            manager,
            view: terminal_view,
        }
    }

    /// Create a new terminal manager for viewing a shared session. See
    /// [`Self::enable_orchestration_polling`] for the meaning of the flag.
    ///
    /// `is_ambient_agent` controls whether the resulting `TerminalView` is
    /// constructed with an `ambient_agent_view_model` up front. Pass `true` when
    /// the pane is known to be an ambient (cloud) run at construction time
    /// (compose panes, restore, and attach-to-running). Shared-session viewers
    /// that only discover the session is ambient at `JoinedSuccessfully` (e.g. a
    /// raw `shared_session` link) pass `false` and get the model created lazily
    /// then via `TerminalView::begin_viewing_ambient_session`.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        session_id: SessionId,
        resources: TerminalViewResources,
        initial_size: Vector2F,
        window_id: WindowId,
        enable_orchestration_polling: bool,
        is_ambient_agent: bool,
        ctx: &mut AppContext,
    ) -> TerminalManagerInit {
        let TerminalManagerInit {
            manager: mut terminal_manager,
            view: terminal_view,
        } = Self::new_internal(
            resources,
            initial_size,
            window_id,
            enable_orchestration_polling,
            is_ambient_agent,
            ctx,
        );

        terminal_manager.connect_session(
            session_id,
            SharedSessionInitialLoadMode::ReplaceFromSessionScrollback,
            ctx,
        );

        TerminalManagerInit {
            manager: terminal_manager,
            view: terminal_view,
        }
    }

    /// Create a new terminal manager for eventually viewing a cloud mode
    /// shared session that is not yet available. See
    /// [`Self::enable_orchestration_polling`] for the meaning of the flag.
    pub fn new_deferred(
        resources: TerminalViewResources,
        initial_size: Vector2F,
        window_id: WindowId,
        enable_orchestration_polling: bool,
        ctx: &mut AppContext,
    ) -> TerminalManagerInit {
        Self::new_internal(
            resources,
            initial_size,
            window_id,
            enable_orchestration_polling,
            true, // is_ambient_agent
            ctx,
        )
    }

    /// Connects a deferred terminal manager to a shared session.
    /// This can only be called on a TerminalManager created with `new_deferred`.
    /// Returns `true` if the connection was initiated, `false` if already connected.
    ///
    /// `append_followup_scrollback` controls whether the initial join uses
    /// `AppendFollowupScrollback` mode instead of `ReplaceFromSessionScrollback`.
    /// Local-to-cloud handoff panes set this to `true` so the pre-populated
    /// forked conversation is not replaced by the cloud session's replay
    /// scrollback.
    pub fn connect_to_session(
        &mut self,
        session_id: SessionId,
        append_followup_scrollback: bool,
        ctx: &mut AppContext,
    ) -> bool {
        let load_mode = if append_followup_scrollback {
            SharedSessionInitialLoadMode::AppendFollowupScrollback
        } else {
            SharedSessionInitialLoadMode::ReplaceFromSessionScrollback
        };
        match self.network_state {
            NetworkState::Idle => {
                self.connect_session(session_id, load_mode, ctx);
                true
            }
            NetworkState::Connecting => {
                log::warn!("connect_to_session called while already connecting to shared session");
                false
            }
            NetworkState::Active(_) => false,
        }
    }

    pub fn attach_execution_session(
        &mut self,
        session_id: SessionId,
        ctx: &mut AppContext,
    ) -> bool {
        match std::mem::replace(&mut self.network_state, NetworkState::Connecting) {
            NetworkState::Active(network) => {
                network.update(ctx, |network, _| {
                    network.close_without_reconnection();
                });
                self.model
                    .lock()
                    .clear_write_to_pty_events_for_shared_session_tx();
                *self.current_network.lock() = None;
                self.network_state = NetworkState::Idle;
            }
            NetworkState::Idle => {
                self.network_state = NetworkState::Idle;
            }
            NetworkState::Connecting => {
                self.network_state = NetworkState::Connecting;
                log::warn!(
                    "attach_execution_session called while already connecting to shared session"
                );
                return false;
            }
        }
        self.connect_session(
            session_id,
            SharedSessionInitialLoadMode::AppendFollowupScrollback,
            ctx,
        );
        self.start_cloud_mode_setup_command_tracking();
        true
    }

    pub fn start_cloud_mode_setup_command_tracking(&mut self) {
        if FeatureFlag::CloudModeSetupV2.is_enabled() {
            self.model
                .lock()
                .block_list_mut()
                .set_is_executing_oz_environment_startup_commands(true);
        }
    }

    /// Connects this terminal manager to a shared session.
    /// This method sets up the network model and all associated event handlers.
    fn connect_session(
        &mut self,
        session_id: SessionId,
        initial_load_mode: SharedSessionInitialLoadMode,
        ctx: &mut AppContext,
    ) {
        match std::mem::replace(&mut self.network_state, NetworkState::Connecting) {
            NetworkState::Idle => {}
            other => {
                self.network_state = other;
                log::warn!("connect_session called on already-connected TerminalManager");
                return;
            }
        }

        // Set up the channel for forwarding write-to-pty events over the network to the sharer.
        // Whenever the user writes to a long-running command (e.g. ctrl-c or typing), those bytes
        // are sent from the terminal view through this channel to the network.
        let (write_to_pty_events_tx, write_to_pty_events_rx) = async_channel::unbounded();
        self.model
            .lock()
            .set_write_to_pty_events_for_shared_session_tx(write_to_pty_events_tx);
        self.model
            .lock()
            .set_shared_session_status(SharedSessionStatus::ViewPending);

        let network = ctx.add_model(|ctx| {
            Network::new(
                session_id,
                self.network_resources.channel_event_proxy.clone(),
                self.view.downgrade(),
                self.model.clone(),
                write_to_pty_events_rx,
                initial_load_mode,
                self.viewer_remote_update_guard.clone(),
                ctx,
            )
        });
        *self.current_network.lock() = Some(network.clone());

        Self::handle_network_events(
            &network,
            &self.view,
            self.model.clone(),
            self.current_network.clone(),
            self.network_resources.prompt_type.clone(),
            self.viewer_remote_update_guard.clone(),
            ctx,
        );
        if !self.outbound_handlers_registered {
            Self::handle_view_events(
                self.current_network.clone(),
                &self.view,
                self.model.clone(),
                self.viewer_remote_update_guard.clone(),
                ctx,
            );
            Self::handle_network_status_events(&self.view, self.current_network.clone(), ctx);

            // LOCAL FORK: the viewer's outbound agent-state mirroring (selected LLM,
            // agent input mode, selected conversation, auto-approve, CLI agent rich
            // input) all subscribed to models that came out with the agent, so the
            // whole outbound block is gone. Terminal-side viewing (PTY, view events,
            // network status) is unaffected.

            self.outbound_handlers_registered = true;
        }
        self.network_state = NetworkState::Active(network);
    }

    // Aggregating these into a struct would just shift the same set of
    // fields into another type purely to placate Clippy without any
    // readability win, since the closure body still needs each clone
    // individually. Suppress the lint instead.
    #[allow(clippy::too_many_arguments)]
    fn handle_network_events(
        network: &ModelHandle<Network>,
        view: &ViewHandle<TerminalView>,
        model: Arc<FairMutex<TerminalModel>>,
        current_network: Arc<FairMutex<Option<ModelHandle<Network>>>>,
        prompt_type: ModelHandle<PromptType>,
        viewer_remote_update_guard: RemoteUpdateGuard,
        ctx: &mut AppContext,
    ) {
        // We use a weak view handle instead of a strong reference because we may add a subscription to the view which moves a strong reference of the Model into the callback,
        // which would create a reference cycle and cause a memory leak. Instead, upgrade the weak view handle lazily.
        let weak_view_handle = view.downgrade();

        ctx.subscribe_to_model(network, move |network, event, ctx| match event {
            NetworkEvent::JoinedSuccessfully {
                active_prompt,
                viewer_id,
                viewer_firebase_uid,
                participant_list,
                input_replica_id,
                universal_developer_input_context,
                source,
            } => {
                model.lock().set_shared_session_source(source.clone());

                Self::handle_active_prompt_update(
                    model.clone(),
                    prompt_type.clone(),
                    weak_view_handle.clone(),
                    active_prompt,
                    ctx,
                );

                // LOCAL FORK: every limb of the universal developer input context
                // applied on join (selected model, agent input mode, CLI agent
                // state, selected conversation) drove agent state and came out with
                // the agent, as did the ambient-session registration and the
                // orchestration viewer model.
                let _ = universal_developer_input_context;

                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };

                let session_id = network.as_ref(ctx).session_id();
                Manager::handle(ctx).update(ctx, |manager, ctx| {
                    manager.joined_share(weak_view_handle.clone(), session_id, ctx);
                });

                view.update(ctx, |terminal_view, ctx| {
                    // LOCAL FORK: `begin_viewing_ambient_session` went with the agent.
                    terminal_view.on_session_share_joined(
                        viewer_id.clone(),
                        *viewer_firebase_uid,
                        input_replica_id.clone(),
                        participant_list.clone(),
                        session_id,
                        source.source_type.clone(),
                        ctx,
                    );
                });

                #[cfg(target_family = "wasm")]
                crate::platform::wasm::emit_event(crate::platform::wasm::WarpEvent::SessionJoined);
            }
            NetworkEvent::SessionEnded { reason } => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };
                let is_ambient_agent = model.lock().is_shared_ambient_agent_session();
                if !Self::handle_viewer_session_end(
                    &view,
                    model.clone(),
                    &current_network,
                    &network,
                    is_ambient_agent,
                    ctx,
                ) {
                    return;
                }
                view.update(ctx, |terminal_view, ctx| {
                    let reason_string = session_ended_reason_string(reason);
                    match reason {
                        SessionEndedReason::EndedBySharer
                        | SessionEndedReason::ExceededSizeLimit => {}
                        SessionEndedReason::InactivityLimitReached => {
                            terminal_view.show_persistent_toast(
                                reason_string,
                                ToastFlavor::Error,
                                ctx,
                            );
                        }
                        SessionEndedReason::InternalServerError if is_ambient_agent => {
                            // Don't show toast for cloud mode sessions - the error message
                            // "ask sharer to reshare" doesn't apply.
                        }
                        _ => {
                            terminal_view.show_persistent_toast(
                                reason_string,
                                ToastFlavor::Error,
                                ctx,
                            );
                        }
                    }
                });
            }
            NetworkEvent::ViewerRemoved { reason } => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };
                // Viewer access was removed and the network will not re-attach.
                // Ambient agent panes route through the resumable
                // execution-ended path (clearing the now-dead live session) so
                // an owner can still start a cloud follow-up; non-ambient
                // viewers fall back to the generic finished-viewer teardown.
                let is_ambient_agent = model.lock().is_shared_ambient_agent_session();
                if !Self::handle_viewer_session_end(
                    &view,
                    model.clone(),
                    &current_network,
                    &network,
                    is_ambient_agent,
                    ctx,
                ) {
                    return;
                }
                view.update(ctx, |terminal_view, ctx| {
                    let reason_string = viewer_removed_reason_string(reason);
                    terminal_view.show_persistent_toast(reason_string, ToastFlavor::Error, ctx);
                });
            }
            NetworkEvent::FailedToJoin { reason } => {
                let session_id = network.as_ref(ctx).session_id();
                log::warn!(
                    "viewer TerminalManager: NetworkEvent::FailedToJoin \
                     session_id={session_id} reason={reason:?}; pane stays in ViewPending \
                     until manual retry or a fresh ensure_shared_session_viewer_child_pane"
                );
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };
                view.update(ctx, |terminal_view, ctx| {
                    terminal_view.show_persistent_toast(
                        reason.user_facing_error_message().to_string(),
                        ToastFlavor::Error,
                        ctx,
                    );
                });
            }
            NetworkEvent::FailedToReconnect => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };
                // Reconnection has been abandoned. Ambient agent panes route
                // through the resumable execution-ended path (which clears the
                // stale live-session state so an owner can start a cloud
                // follow-up); non-ambient viewers fall back to the generic
                // finished-viewer teardown.
                let is_ambient_agent = model.lock().is_shared_ambient_agent_session();
                if !Self::handle_viewer_session_end(
                    &view,
                    model.clone(),
                    &current_network,
                    &network,
                    is_ambient_agent,
                    ctx,
                ) {
                    return;
                }
                // Ambient panes surface the resumable tombstone / follow-up
                // input instead, so the generic "please try again" toast (which
                // implies a retryable transport error) would be misleading.
                if !is_ambient_agent {
                    view.update(ctx, |terminal_view, ctx| {
                        terminal_view.show_persistent_toast(
                            "Failed to reconnect. Please try again later.".to_owned(),
                            ToastFlavor::Error,
                            ctx,
                        );
                    });
                }
            }
            NetworkEvent::SharerActivePromptUpdated(active_prompt_update) => {
                Self::handle_active_prompt_update(
                    model.clone(),
                    prompt_type.clone(),
                    weak_view_handle.clone(),
                    &active_prompt_update.active_prompt,
                    ctx,
                );
            }
            NetworkEvent::UniversalDeveloperInputContextUpdated(context_update) => {
                // LOCAL FORK: the selected model, agent input mode, selected
                // conversation, auto-approve and CLI agent limbs of this update all
                // drove agent state and came out with the agent. Only the
                // long-running command interaction below still applies.
                //
                // Held for the whole arm so applying an inbound update doesn't echo
                // straight back to the sharer.
                let _active_remote_update = viewer_remote_update_guard.start_remote_update();

                if model
                    .lock()
                    .block_list()
                    .active_block()
                    .is_active_and_long_running()
                {
                    if let Some(interaction) =
                        context_update.long_running_command_agent_interaction.clone()
                    {
                        if let Some(view) = weak_view_handle.upgrade(ctx) {
                            view.update(ctx, |view, ctx| {
                                view.apply_long_running_command_agent_interaction(interaction, ctx);
                            });
                        }
                    } else if let Some(interaction_state) =
                        context_update.long_running_command_agent_interaction_state
                    {
                        // TODO (roland): this is kept around for backward compatibility. Remove after 6 weeks (around Jul 23, 2026) 
                        // once clients have updated to use context_update.long_running_command_agent_interaction above.
                        if let Some(view) = weak_view_handle.upgrade(ctx) {
                            view.update(ctx, |view, ctx| {
                                view.apply_long_running_command_agent_interaction_state(
                                    interaction_state,
                                    None,
                                    ctx,
                                );
                            });
                        }
                    }
                }
            }
            NetworkEvent::Reconnecting => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };
                view.update(ctx, |view, ctx| {
                    view.on_shared_session_reconnection_status_changed(true, ctx)
                });
            }
            NetworkEvent::ParticipantListUpdated(participant_list) => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };

                // A change to our role may have originated from the server,
                // make sure that our own state changes if it does.
                view.update(ctx, |view, ctx| {
                    view.on_self_role_maybe_changed(participant_list.as_ref(), ctx);
                });

                if let Some(presence_manager) = view.as_ref(ctx).shared_session_presence_manager() {
                    presence_manager.update(ctx, |presence_manager, ctx| {
                        presence_manager.update_participants(*participant_list.clone(), ctx)
                    });
                };

                if let Some(session_id) = view.as_ref(ctx).shared_session_id().cloned() {
                    SessionPermissionsManager::handle(ctx).update(
                        ctx,
                        |permissions_manager, ctx| {
                            permissions_manager.updated_guests(
                                ctx,
                                session_id,
                                participant_list.guests.clone(),
                                participant_list.pending_guests.clone(),
                            );
                        },
                    );
                }
            }
            NetworkEvent::ParticipantPresenceUpdated(update) => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };

                view.update(ctx, |view, ctx| {
                    view.on_participant_presence_updated(update, ctx);
                });
            }
            NetworkEvent::ReconnectedSuccessfully => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };

                view.update(ctx, |view, ctx| {
                    view.on_shared_session_reconnection_status_changed(false, ctx)
                });
            }
            NetworkEvent::ParticipantRoleChanged {
                participant_id,
                reason,
                role,
            } => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };

                view.update(ctx, |view, ctx| {
                    view.maybe_show_role_changed_toast(participant_id, *reason, *role, ctx);
                    view.on_participant_role_changed(participant_id, *role, ctx);
                });
            }
            NetworkEvent::InputUpdated {
                block_id,
                operations,
            } => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };

                view.update(ctx, |view, ctx| {
                    // LOCAL FORK: the cloud-mode-startup guard here
                    // (`is_cloud_agent_pre_first_exchange`) needed the ambient agent
                    // view model and agent view controller, both of which came out
                    // with the agent. Without a cloud agent there is no setup phase
                    // to suppress, so remote input updates always apply.
                    view.apply_viewer_shared_session_input_update(block_id, operations.clone(), ctx);
                })
            }
            NetworkEvent::RoleRequestInFlight(role_request_id) => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };

                view.update(ctx, |view, ctx| {
                    view.on_shared_session_viewer_role_request_in_flight(
                        role_request_id.clone(),
                        ctx,
                    );
                });
            }
            NetworkEvent::RoleRequestResponse(role_request_response) => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };

                view.update(ctx, |view, ctx| {
                    view.on_shared_session_role_request_response(
                        role_request_response.clone(),
                        ctx,
                    );
                });
            }
            NetworkEvent::CommandExecutionRequestFailed { reason, .. } => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };
                view.update(ctx, |terminal_view, ctx| {
                    Self::handle_command_execution_request_failed(terminal_view, reason, ctx);
                });
            }
            NetworkEvent::WriteToPtyRequestFailed { reason } => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };
                view.update(ctx, |terminal_view, ctx| {
                    let reason_string = write_to_pty_failure_reason_string(reason);
                    terminal_view.show_persistent_toast(reason_string, ToastFlavor::Error, ctx);
                });
            }
            NetworkEvent::AgentPromptRequestInFlight(_id) => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };
                view.update(ctx, |terminal_view, ctx| {
                    // Restore frozen visual state. optimistically_show_empty=true creates
                    // a display-only empty ephemeral for immediate UX feedback. Unlike a
                    // regular ephemeral, this one discards its content on materialization
                    // instead of restoring it to the regular buffer, so no spurious CRDT
                    // delete ops are generated for concurrent edits by other viewers.
                    terminal_view.input().update(ctx, |input, ctx| {
                        input.unfreeze_agent_input(true, ctx);
                    });
                });
            }
            NetworkEvent::AgentPromptRequestFailed { reason, .. } => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };
                view.update(ctx, |terminal_view, ctx| {
                    let reason_string = agent_prompt_failure_reason_string(reason);
                    terminal_view.show_persistent_toast(reason_string, ToastFlavor::Error, ctx);

                    // Restore frozen visual state without clearing the buffer — the prompt
                    // failed so no CRDT delete ops were sent, and the user should be able
                    // to retry with their original text.
                    terminal_view.input().update(ctx, |input, ctx| {
                        input.unfreeze_agent_input(false, ctx);
                    });
                });
            }
            NetworkEvent::ControlActionRequestFailed { reason } => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };
                view.update(ctx, |terminal_view, ctx| {
                    let reason_string = control_action_failure_reason_string(reason);
                    terminal_view.show_persistent_toast(reason_string, ToastFlavor::Error, ctx);
                });
            }
            NetworkEvent::LinkAccessLevelUpdated { role } => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };
                view.update(ctx, |terminal_view, ctx| {
                    let Some(session_id) = terminal_view.shared_session_id() else {
                        return;
                    };
                    SessionPermissionsManager::handle(ctx).update(
                        ctx,
                        |permissions_manager, ctx| {
                            permissions_manager.updated_link_permissions(*session_id, *role, ctx);
                        },
                    );
                });
            }
            NetworkEvent::TeamAccessLevelUpdated { team_acl } => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };
                view.update(ctx, |terminal_view, ctx| {
                    let Some(session_id) = terminal_view.shared_session_id() else {
                        return;
                    };
                    SessionPermissionsManager::handle(ctx).update(
                        ctx,
                        |permissions_manager, ctx| {
                            permissions_manager.updated_team_permissions(
                                *session_id,
                                team_acl.clone(),
                                ctx,
                            );
                        },
                    );
                });
            }
            NetworkEvent::LinkAccessLevelUpdateResponse { response } => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };
                view.update(ctx, |terminal_view, ctx| match response {
                    LinkAccessLevelUpdateResponse::Ok { role } => {
                        let Some(session_id) = terminal_view.shared_session_id() else {
                            return;
                        };
                        SessionPermissionsManager::handle(ctx).update(
                            ctx,
                            |permissions_manager, ctx| {
                                permissions_manager.updated_link_permissions(
                                    *session_id,
                                    *role,
                                    ctx,
                                );
                            },
                        );
                    }
                    LinkAccessLevelUpdateResponse::Error => {
                        terminal_view.show_persistent_toast(
                            "Failed to update permissions for shared session".to_owned(),
                            ToastFlavor::Error,
                            ctx,
                        );
                    }
                });
            }
            NetworkEvent::TeamAccessLevelUpdateResponse { response } => {
                let Some(view) = weak_view_handle.upgrade(ctx) else {
                    return;
                };
                view.update(ctx, |terminal_view, ctx| match response {
                    TeamAccessLevelUpdateResponse::Success { team_acl, .. } => {
                        let Some(session_id) = terminal_view.shared_session_id() else {
                            return;
                        };
                        SessionPermissionsManager::handle(ctx).update(
                            ctx,
                            |permissions_manager, ctx| {
                                permissions_manager.updated_team_permissions(
                                    *session_id,
                                    team_acl.clone(),
                                    ctx,
                                );
                            },
                        );
                    }
                    TeamAccessLevelUpdateResponse::Error(_) => {
                        terminal_view.show_persistent_toast(
                            "Something went wrong. Please try again.".to_owned(),
                            ToastFlavor::Error,
                            ctx,
                        );
                    }
                });
            }
            NetworkEvent::AddGuestsResponse { response } => {
                if let AddGuestsResponse::Error(reason) = response {
                    let Some(view) = weak_view_handle.upgrade(ctx) else {
                        return;
                    };
                    view.update(ctx, |terminal_view, ctx| {
                        let reason_string = match reason {
                            session_sharing_protocol::common::FailedToAddGuestsReason::NotWarpUsers => {
                                "One or more of the emails are not Warp users.".to_owned()
                            }
                            session_sharing_protocol::common::FailedToAddGuestsReason::GuestAlreadyAdded => {
                                "One or more of the guests has already been added.".to_owned()
                            }
                            _ => "Something went wrong. Please try again.".to_owned(),
                        };
                        terminal_view.show_persistent_toast(reason_string, ToastFlavor::Error, ctx);
                    });
                }
            }
            NetworkEvent::RemoveGuestResponse { response } => {
                if let RemoveGuestResponse::Error(_) = response {
                    let Some(view) = weak_view_handle.upgrade(ctx) else {
                        return;
                    };
                    view.update(ctx, |terminal_view, ctx| {
                        terminal_view.show_persistent_toast(
                            "Something went wrong. Please try again.".to_owned(),
                            ToastFlavor::Error,
                            ctx,
                        );
                    });
                }
            }
            NetworkEvent::UpdatePendingUserRoleResponse { response } => {
                if let UpdatePendingUserRoleResponse::Error(_) = response {
                    let Some(view) = weak_view_handle.upgrade(ctx) else {
                        return;
                    };
                    view.update(ctx, |terminal_view, ctx| {
                        terminal_view.show_persistent_toast(
                            "Something went wrong. Please try again.".to_owned(),
                            ToastFlavor::Error,
                            ctx,
                        );
                    });
                }
            }
        });
    }

    fn handle_active_prompt_update(
        model: Arc<FairMutex<TerminalModel>>,
        prompt_type: ModelHandle<PromptType>,
        weak_view_handle: WeakViewHandle<TerminalView>,
        active_prompt: &ActivePrompt,
        ctx: &mut AppContext,
    ) {
        let mut model = model.lock();
        match active_prompt {
            ActivePrompt::WarpPrompt(serialized_prompt_snapshot) => {
                match serde_json::from_str::<PromptSnapshot>(serialized_prompt_snapshot) {
                    Ok(prompt_snapshot) => {
                        model.block_list_mut().set_honor_ps1(false);
                        // Overwrite the static prompt with the new snapshot.
                        prompt_type.update(ctx, |prompt_type, ctx| {
                            if let PromptType::Static { snapshot } = prompt_type {
                                *snapshot = prompt_snapshot;
                                ctx.notify();
                            } else {
                                log::warn!("Received ActivePrompt::WarpPrompt updated but prompt type is not Static");
                            }
                        });
                    }
                    Err(e) => {
                        report_error!(anyhow::Error::new(e).context(
                            "Failed to deserialize prompt snapshot from shared session server"
                        ))
                    }
                }
            }
            ActivePrompt::PS1 => {
                // The viewer already receives bytes from the pty for the PS1 prompt, so we only need to choose to render it.
                model.block_list_mut().set_honor_ps1(true);
            }
        }
        let Some(view) = weak_view_handle.upgrade(ctx) else {
            return;
        };
        // This is needed to re-render the input if we changed prompt types.
        view.update(ctx, |view, ctx| {
            view.input().update(ctx, |input, ctx| {
                input.notify_and_notify_children(ctx);
            })
        });
    }

    // LOCAL FORK: fns handle_selected_agent_model_update, handle_input_mode_update
    // and handle_selected_conversation_update removed with the agent. Each applied
    // one limb of the sharer's agent state (selected LLM, agent input mode, selected
    // conversation) onto the local agent models, all of which are gone.

    fn handle_view_events(
        current_network: Arc<FairMutex<Option<ModelHandle<Network>>>>,
        view: &ViewHandle<TerminalView>,
        model: Arc<FairMutex<TerminalModel>>,
        viewer_remote_update_guard: RemoteUpdateGuard,
        ctx: &mut AppContext,
    ) {
        ctx.subscribe_to_view(view, move |view, event, ctx| match event {
            TerminalViewEvent::SelectedBlocksChanged | TerminalViewEvent::SelectedTextChanged => {
                let selection = view.read(ctx, |view, ctx| {
                    view.get_shared_session_presence_selection(ctx)
                });
                Self::update_current_network(&current_network, ctx, |network, _| {
                    network.send_presence_selection_if_changed(selection);
                });
            }
            TerminalViewEvent::RequestSharedSessionRole(role) => {
                Self::update_current_network(&current_network, ctx, |network, _| {
                    network.send_role_request(*role);
                });
            }
            TerminalViewEvent::CancelRoleRequest(role_request_id) => {
                Self::update_current_network(&current_network, ctx, |network, _| {
                    network.send_cancel_role_request(role_request_id.clone());
                });
            }
            TerminalViewEvent::InputEditorUpdated {
                block_id,
                operations,
            } => {
                let should_send_input_update = view.read(ctx, |view, ctx| {
                    let model = model.lock();
                    model.block_list().active_block_id() == block_id
                        && model.shared_session_status().is_executor()
                        && view.should_publish_shared_session_input_editor_update(&model, ctx)
                });
                if should_send_input_update {
                    Self::update_current_network(&current_network, ctx, |network, _| {
                        network.send_input_update(block_id, operations.iter());
                    });
                }
            }
            TerminalViewEvent::ExecuteCommand(ExecuteCommandEvent {
                command, source, ..
            }) => {
                // For a viewer, only the SharedSession execution source is valid.
                let CommandExecutionSource::SharedSession { block_id, .. } = source
                else {
                    log::warn!("Got a TerminalViewEvent::ExecuteCommand in viewer::TerminalManager where the source was not SharedSession");
                    return;
                };

                // If the block ID has become stale by the time we get here,
                // we don't need to send this update to the server.
                if model.lock().block_list().active_block_id() != block_id {
                    return;
                }

                // Only send command execution request if the viewer is an executor.
                if model.lock().shared_session_status().is_executor() {
                    Self::update_current_network(&current_network, ctx, |network, _| {
                        network.send_command_execution_request(block_id, command.to_owned());
                    });
                }
            }
            TerminalViewEvent::RejoinCurrentSession => {
                Self::update_current_network(&current_network, ctx, |network, ctx| {
                    network.reauthenticate_viewer(ctx);
                });
            }
            TerminalViewEvent::SendAgentPrompt {
                server_conversation_token,
                prompt,
                attachments,
            } => {
                Self::update_current_network(&current_network, ctx, |network, _| {
                    network.send_agent_prompt_request(
                        *server_conversation_token,
                        prompt.clone(),
                        attachments.clone(),
                    );
                });
            }
            TerminalViewEvent::CancelSharedSessionConversation {
                server_conversation_token,
            } => {
                Self::update_current_network(&current_network, ctx, |network, _| {
                    network.send_cancel_control_action(*server_conversation_token);
                });
            }
            TerminalViewEvent::ReportViewerTerminalSize { window_size } => {
                Self::update_current_network(&current_network, ctx, |network, _| {
                    network.send_report_terminal_size(*window_size);
                });
            }
            TerminalViewEvent::LongRunningCommandAgentInteractionStateChanged {
                state,
                block_id,
            } => {
                let interaction =
                    block_id
                        .clone()
                        .map(|block_id| LongRunningCommandAgentInteraction {
                            block_id: block_id.into(),
                            state: *state,
                        });
                Self::send_input_context_update_to_current_network(
                    &viewer_remote_update_guard,
                    &model,
                    &current_network,
                    UniversalDeveloperInputContextUpdate {
                        long_running_command_agent_interaction_state: Some(*state),
                        long_running_command_agent_interaction: interaction,
                        ..Default::default()
                    },
                    ctx,
                );
            }
            TerminalViewEvent::UpdateSessionLinkPermissions { role } => {
                Self::update_current_network(&current_network, ctx, |network, _| {
                    network.send_link_permission_update(*role);
                });
            }
            TerminalViewEvent::UpdateSessionTeamPermissions { role, team_uid } => {
                Self::update_current_network(&current_network, ctx, |network, _| {
                    network.send_team_permission_update(*role, team_uid.clone());
                });
            }
            TerminalViewEvent::AddGuests { emails, role } => {
                Self::update_current_network(&current_network, ctx, |network, _| {
                    network.send_add_guests(emails.clone(), *role);
                });
            }
            TerminalViewEvent::RemoveGuest { user_uid } => {
                Self::update_current_network(&current_network, ctx, |network, _| {
                    network.send_remove_guest(*user_uid);
                });
            }
            TerminalViewEvent::RemovePendingGuest { email } => {
                Self::update_current_network(&current_network, ctx, |network, _| {
                    network.send_remove_pending_guest(email.clone());
                });
            }
            TerminalViewEvent::UpdateUserRole { user_uid, role } => {
                Self::update_current_network(&current_network, ctx, |network, _| {
                    network.send_user_role_update(*user_uid, *role);
                });
            }
            TerminalViewEvent::UpdatePendingUserRole { email, role } => {
                Self::update_current_network(&current_network, ctx, |network, _| {
                    network.send_pending_user_role_update(email.clone(), *role);
                });
            }
            _ => (),
        });
    }

    fn handle_network_status_events(
        view: &ViewHandle<TerminalView>,
        current_network: Arc<FairMutex<Option<ModelHandle<Network>>>>,
        ctx: &mut AppContext,
    ) {
        let weak_view_handle = view.downgrade();
        let network_status = NetworkStatus::handle(ctx);

        ctx.subscribe_to_model(&network_status, move |_, event, ctx| {
            let Some(view) = weak_view_handle.upgrade(ctx) else {
                return;
            };
            let NetworkStatusEvent::NetworkStatusChanged { new_status } = event;
            match new_status {
                NetworkStatusKind::Online => {
                    if Self::current_network(&current_network)
                        .is_some_and(|network| network.as_ref(ctx).is_connected())
                    {
                        view.update(ctx, |view, ctx| {
                            view.on_shared_session_reconnection_status_changed(false, ctx)
                        });
                    }
                }
                NetworkStatusKind::Offline => {
                    view.update(ctx, |view, ctx| {
                        view.on_shared_session_reconnection_status_changed(true, ctx)
                    });
                }
            }
        });
    }

    // LOCAL FORK: fn stop_orchestration_polling removed with the agent, along with
    // the OrchestrationViewerModel it dropped and the OrchestrationEventStreamer
    // registration it released.

    /// Common teardown for the viewer session-end network events
    /// (`SessionEnded`, `ViewerRemoved`, `FailedToReconnect`).
    ///
    /// Ambient agent panes route through [`Self::end_current_ambient_session`],
    /// which clears the live `active_execution_session_id`, records the ended
    /// session, and surfaces the resumable tombstone / follow-up input — so a
    /// session lost via reconnect failure or access removal lands in the same
    /// editable post-run state as a clean `SessionEnded`, rather than leaving a
    /// stale "session is live" gate that misroutes follow-ups to a local agent.
    /// Non-ambient panes use the generic [`Self::shared_session_ended`]
    /// finished-viewer teardown.
    ///
    /// Returns `false` when an ambient end was ignored because the ended network
    /// is no longer the current one (a stale event); callers should bail without
    /// surfacing an end-of-session toast.
    ///
    /// LOCAL FORK: the `orchestration_viewer_model` parameter and the
    /// owner/non-owner polling teardown came out with the agent.
    fn handle_viewer_session_end(
        terminal_view: &ViewHandle<TerminalView>,
        model: Arc<FairMutex<TerminalModel>>,
        current_network: &Arc<FairMutex<Option<ModelHandle<Network>>>>,
        ended_network: &ModelHandle<Network>,
        is_ambient_agent: bool,
        ctx: &mut AppContext,
    ) -> bool {
        if is_ambient_agent {
            if !Self::end_current_ambient_session(
                terminal_view,
                model,
                current_network,
                ended_network,
                ctx,
            ) {
                return false;
            }
        } else {
            Self::shared_session_ended(terminal_view, model, ctx);
        }
        true
    }

    fn shared_session_ended(
        terminal_view: &ViewHandle<TerminalView>,
        model: Arc<FairMutex<TerminalModel>>,
        ctx: &mut AppContext,
    ) {
        let terminal_view_id = terminal_view.id();

        // LOCAL FORK: cancelling the viewer's in-progress agent conversations on
        // session end went out with the agent's conversation history model.

        Manager::handle(ctx).update(ctx, |manager, _| {
            manager.left_share(terminal_view_id);
        });

        terminal_view.update(ctx, |terminal_view, ctx| {
            terminal_view.on_session_share_ended(ctx);
        });

        model
            .lock()
            .set_shared_session_status(SharedSessionStatus::FinishedViewer);
        model
            .lock()
            .clear_write_to_pty_events_for_shared_session_tx();
    }

    fn end_current_ambient_session(
        terminal_view: &ViewHandle<TerminalView>,
        model: Arc<FairMutex<TerminalModel>>,
        current_network: &Arc<FairMutex<Option<ModelHandle<Network>>>>,
        ended_network: &ModelHandle<Network>,
        ctx: &mut AppContext,
    ) -> bool {
        let ended_session_id = ended_network.as_ref(ctx).session_id();
        if !Self::current_network(current_network)
            .is_some_and(|network| network.as_ref(ctx).session_id() == ended_session_id)
        {
            return false;
        }
        Manager::handle(ctx).update(ctx, |manager, _| {
            manager.left_share(terminal_view.id());
        });

        model
            .lock()
            .clear_write_to_pty_events_for_shared_session_tx();
        if FeatureFlag::HandoffCloudCloud.is_enabled() {
            // LOCAL FORK: task ownership and the ambient agent view model went out
            // with the agent, so there is no longer an "owner keeps an editable
            // Cloud Mode pane" case — every ended ambient session lands in the
            // read-only finished-viewer state.
            model
                .lock()
                .set_shared_session_status(SharedSessionStatus::FinishedViewer);
            terminal_view.update(ctx, |terminal_view, ctx| {
                terminal_view.on_ambient_agent_execution_ended(ctx);
            });
        }
        if Self::current_network(current_network)
            .is_some_and(|network| network.as_ref(ctx).session_id() == ended_session_id)
        {
            *current_network.lock() = None;
        }
        true
    }
}

impl crate::terminal::TerminalManager for TerminalManager {
    fn model(&self) -> Arc<FairMutex<TerminalModel>> {
        self.model.clone()
    }

    fn on_view_detached(&self, detach_type: DetachType, app: &mut AppContext) {
        // Keep the network + shared-session state — and the orchestration
        // viewer model (OVM) — alive for non-permanent detaches:
        // - `HiddenForClose`: the pane may be restored from the undo-close stack within the
        //   grace window (~60s default). We deliberately leave the OVM (and its ancestor
        //   streamer registration) in place so undo-close-tab restores the pill bar
        //   seamlessly. If the tab is never restored, we'll be invoked again with `Closed`
        //   from the grace-period expiry and tear down then.
        // - `Moved`: the same `TerminalManager` is reused in the target pane group (the
        //   `Box<dyn AnyPaneContent>` is transferred via `remove_pane_for_move` and then
        //   immediately re-attached), so tearing down the network or OVM would break the
        //   live session.
        // Only `Closed` tears down the OVM here.
        if !matches!(detach_type, DetachType::Closed) {
            return;
        }

        // LOCAL FORK: the ActiveAgentViewsModel unregistration and the
        // orchestration viewer model teardown both came out with the agent.

        if let NetworkState::Active(ref network) = self.network_state {
            network.update(app, |network, _| {
                network.close_without_reconnection();
            });
        }
        self.model
            .lock()
            .set_shared_session_status(SharedSessionStatus::FinishedViewer);
        self.view
            .update(app, |view, ctx| view.on_session_share_ended(ctx));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
#[path = "terminal_manager_tests.rs"]
mod tests;
