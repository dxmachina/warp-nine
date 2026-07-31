//! Implementation of terminal panes.
use std::sync::mpsc::SyncSender;

use url::Url;
use warp_core::execution_mode::AppExecutionMode;
use warp_errors::report_error;
use warpui::{
    AppContext, EntityId, ModelHandle, SingletonEntity, ViewContext, ViewHandle, WindowId,
};

#[cfg(not(target_family = "wasm"))]
use super::{
    DetachType, PaneConfiguration, PaneContent, PaneId, PaneStackEvent, PaneView, ShareableLink,
    ShareableLinkError, TerminalPaneId,
};
// LOCAL FORK: the conversation, execution profile, ambient task and child agent models
// this pane wired up all came out with the agent.
use crate::app_state::{LeafContents, TerminalPaneSnapshot};
use crate::code::buffer_location::LocalOrRemotePath;
#[cfg(feature = "local_fs")]
use crate::pane_group::CodeSource;
use crate::pane_group::Event::OpenConversationHistory;
use crate::pane_group::{self, Direction, PaneGroup};
use crate::persistence::{BlockCompleted, ModelEvent};
use crate::session_management::SessionNavigationData;
use crate::terminal::general_settings::GeneralSettings;
use crate::terminal::shared_session::manager::{Manager, ManagerEvent};
use crate::terminal::shared_session::role_change_modal::RoleChangeOpenSource;
use crate::terminal::shared_session::{SharedSessionStatus, join_link};
use crate::terminal::view::Event;
use crate::terminal::{TerminalManager, TerminalView};
use crate::view_components::ToastFlavor;
use crate::workspace::PaneViewLocator;
use crate::workspace::sync_inputs::SyncedInputState;

pub type TerminalPaneView = PaneView<TerminalView>;

/// Data kept for terminal panes.
pub struct TerminalPane {
    model_event_sender: Option<SyncSender<ModelEvent>>,

    /// Used to uniquely identify the pane, even across separate runs of the app.
    uuid: Vec<u8>,

    pane_configuration: ModelHandle<PaneConfiguration>,

    /// Defining `terminal_manager` before `view` means that `terminal_manager`
    /// gets dropped first (guaranteed by the language), which halts the event
    /// loop and avoids possible deadlocks during session cleanup. This is enforced
    /// by the `PaneStack`, since the terminal manager is the associated data for
    /// the backing pane view.
    view: ViewHandle<TerminalPaneView>,
}

// LOCAL FORK: fn host_terminal_shared_session_source_type and
// fn inherit_share_for_local_child removed with the agent; they only existed to
// cascade a host's shared session onto `run_agents(local)` child panes.

impl TerminalPane {
    pub(in crate::pane_group) fn new(
        uuid: Vec<u8>,
        terminal_manager: ModelHandle<Box<dyn TerminalManager>>,
        terminal_view: ViewHandle<TerminalView>,
        model_event_sender: Option<SyncSender<ModelEvent>>,
        ctx: &mut ViewContext<PaneGroup>,
    ) -> Self {
        let pane_configuration = terminal_view.as_ref(ctx).pane_configuration().to_owned();
        let view = ctx.add_typed_action_view(|ctx| {
            let pane_id = PaneId::from_terminal_pane_ctx(ctx);
            PaneView::new(
                pane_id,
                terminal_view,
                terminal_manager,
                pane_configuration.clone(),
                ctx,
            )
        });

        Self {
            model_event_sender,
            uuid,
            pane_configuration,
            view,
        }
    }

    /// The [`PaneView<TerminalView>`] for this pane.
    #[cfg(any(test, feature = "integration_tests"))]
    pub(in crate::pane_group) fn pane_view(&self) -> ViewHandle<TerminalPaneView> {
        self.view.to_owned()
    }

    /// The [`TerminalView`] backing the [`PaneView`] for this terminal pane.
    pub(crate) fn terminal_view(&self, ctx: &AppContext) -> ViewHandle<TerminalView> {
        self.view.as_ref(ctx).child(ctx)
    }

    /// The UUID that identifies this terminal session across app restarts.
    pub(in crate::pane_group) fn session_uuid(&self) -> Vec<u8> {
        self.uuid.clone()
    }

    /// The terminal manager responsible for this session's event loop.
    pub(in crate::pane_group) fn terminal_manager(
        &self,
        ctx: &AppContext,
    ) -> ModelHandle<Box<dyn TerminalManager>> {
        self.view.as_ref(ctx).child_data(ctx).clone()
    }

    /// Instructs the SQLite thread to delete blocks for this session.
    pub(in crate::pane_group) fn delete_blocks(&self, ctx: &AppContext) {
        if !AppExecutionMode::as_ref(ctx).can_save_session() {
            return;
        }

        if let Some(sender) = &self.model_event_sender {
            let model_event = ModelEvent::DeleteBlocks(self.uuid.clone());
            if let Err(err) = sender.send(model_event) {
                report_error!(
                    anyhow::Error::new(err).context("Error sending blocks deleted event"),
                    extra: { "terminal_id" => ?self.terminal_view(ctx).id() }
                );
            }
        }
    }

    pub fn session_navigation_data(
        &self,
        pane_group_id: EntityId,
        window_id: WindowId,
        app: &AppContext,
    ) -> SessionNavigationData {
        let view = self.terminal_view(app).as_ref(app);
        SessionNavigationData::new(
            view.full_prompt(app),
            view.prompt_elements(app),
            view.session_command_context(app),
            PaneViewLocator {
                pane_group_id,
                pane_id: self.id(),
            },
            view.last_focus_ts(),
            view.is_read_only(),
            window_id,
            view.model.lock().shared_session_status().clone(),
        )
    }

    pub fn terminal_pane_id(&self) -> TerminalPaneId {
        self.id()
            .as_terminal_pane_id()
            .expect("Should be able to derive a TerminalPaneId from TerminalPane")
    }
}

impl PaneContent for TerminalPane {
    fn id(&self) -> PaneId {
        PaneId::from_terminal_pane_view(&self.view)
    }

    fn attach(
        &self,
        group: &PaneGroup,
        focus_handle: crate::pane_group::focus_state::PaneFocusHandle,
        ctx: &mut ViewContext<PaneGroup>,
    ) {
        // TODO(ben): As much as possible, logic from PaneGroup::add_session should go here.
        //  This will simplify PaneGroup, especially when implementing pane management.
        let terminal_pane_id = self.terminal_pane_id();

        self.view
            .update(ctx, |view, ctx| view.set_focus_handle(focus_handle, ctx));

        // Attach the initial terminal view in the stack.
        attach_terminal_view(&self.terminal_view(ctx), terminal_pane_id, ctx);

        // Subscribe to the pane stack to handle views being pushed/popped.
        let pane_stack = self.view.as_ref(ctx).pane_stack().clone();
        ctx.subscribe_to_model(&pane_stack, move |group, _, event, ctx| {
            handle_pane_stack_event(group, event, terminal_pane_id, ctx);
        });

        ctx.subscribe_to_view(&self.view, move |group, _, event, ctx| {
            group.handle_pane_view_event(terminal_pane_id.into(), event, ctx);
        });

        if SyncedInputState::as_ref(ctx).should_sync_this_pane_group(ctx.view_id(), ctx.window_id())
            && let Some(active_pane_view) = group.active_session_view(ctx)
        {
            let event = active_pane_view
                .as_ref(ctx)
                .create_sync_event_based_on_terminal_state(ctx);

            group.send_sync_event_to_session(terminal_pane_id, &event, ctx);
        }

        let terminal_view_id = self.terminal_view(ctx).id();
        let manager_model = Manager::handle(ctx);
        ctx.subscribe_to_model(&manager_model, move |group, model_handle, event, ctx| {
            if let ManagerEvent::JoinedSession {
                session_id: _,
                view_id,
            } = event
            {
                // only take action if the view id is ours
                if *view_id == terminal_view_id {
                    let url = retrieve_shared_session_link(model_handle.as_ref(ctx), view_id);
                    group.handle_pane_link_updated(terminal_pane_id.into(), url, ctx);
                }
            }
        });

        // LOCAL FORK: the agent conversation history subscription, the agent view
        // controller wiring and the active agent views registration all went with the
        // agent.
    }

    fn detach(
        &self,
        _group: &PaneGroup,
        detach_type: DetachType,
        ctx: &mut ViewContext<PaneGroup>,
    ) {
        if matches!(detach_type, DetachType::Closed) {
            // Only delete blocks if the session is being permanently closed.
            // LOCAL FORK: clearing the surface's agent conversations went with the agent.
            self.delete_blocks(ctx);
        }

        // Unsubscribe from all views in the pane stack.
        let pane_stack = self.view.as_ref(ctx).pane_stack().clone();
        let contents = pane_stack.as_ref(ctx).entries().to_vec();
        for (manager, view) in contents {
            // Notify the view that it's being detached so it can react appropriately
            // (e.g. the shared-session viewer tears down its network only when the detach
            // is not reversible).
            manager.update(ctx, |terminal_manager, ctx| {
                terminal_manager.on_view_detached(detach_type, ctx);
            });
            ctx.unsubscribe_to_view(&view);
        }

        // LOCAL FORK: unregistering the agent view controller, the ambient session and
        // the CLI agent session all went with the agent.

        ctx.unsubscribe_to_model(&pane_stack);

        ctx.unsubscribe_to_view(&self.view);

        ctx.unsubscribe_to_model(&Manager::handle(ctx));
    }

    fn snapshot(&self, app: &AppContext) -> LeafContents {
        let view = self.terminal_view(app).as_ref(app);
        let is_active = view.is_active_session(app);

        // LOCAL FORK: ambient agent panes, conversation transcript viewers, the agent
        // input config, the base model override and the active execution profile all
        // came out with the agent, so every terminal pane now snapshots as a plain
        // terminal.
        if view.model.lock().shared_session_status().is_viewer() {
            // Shared-session viewers have no local shell state worth restoring.
            LeafContents::Terminal(TerminalPaneSnapshot {
                uuid: self.uuid.clone(),
                cwd: None,
                is_active,
                is_read_only: false,
                shell_launch_data: None,
                llm_model_override: None,
                active_profile_id: None,
            })
        } else {
            LeafContents::Terminal(TerminalPaneSnapshot {
                uuid: self.uuid.clone(),
                cwd: view.pwd_if_local(app),
                is_active,
                is_read_only: view.model.lock().is_read_only(),
                shell_launch_data: view.shell_launch_data_if_local(app),
                llm_model_override: None,
                active_profile_id: None,
            })
        }
    }

    fn has_application_focus(&self, ctx: &mut ViewContext<PaneGroup>) -> bool {
        self.view.is_self_or_child_focused(ctx)
    }

    fn focus(&self, ctx: &mut ViewContext<PaneGroup>) {
        self.terminal_view(ctx)
            .update(ctx, |view, ctx| view.redetermine_global_focus(ctx));
    }

    fn shareable_link(
        &self,
        ctx: &mut ViewContext<PaneGroup>,
    ) -> Result<ShareableLink, ShareableLinkError> {
        let manager = self.terminal_manager(ctx);
        let the_model = manager.as_ref(ctx).model();
        let lock = the_model.lock();

        // LOCAL FORK: a conversation transcript viewer used to resolve its share link from
        // the agent's history model; that pane kind no longer exists.

        // Check for shared session status
        let session_status = lock.shared_session_status();
        match session_status {
            SharedSessionStatus::NotShared => Ok(ShareableLink::Base),
            SharedSessionStatus::ActiveViewer { role: _ } => {
                let manager = Manager::as_ref(ctx);
                let terminal_view_id = self.terminal_view(ctx).id();
                if let Some(url) = retrieve_shared_session_link(manager, &terminal_view_id) {
                    Ok(ShareableLink::Pane { url })
                } else {
                    Err(ShareableLinkError::Unexpected(String::from(
                        "Failed to retreive shared session link",
                    )))
                }
            }
            _ => Err(ShareableLinkError::Expected),
        }
    }

    fn pane_configuration(&self) -> ModelHandle<PaneConfiguration> {
        self.pane_configuration.clone()
    }

    fn is_pane_being_dragged(&self, ctx: &AppContext) -> bool {
        self.view.as_ref(ctx).is_being_dragged()
    }
}

fn retrieve_shared_session_link(manager: &Manager, terminal_view_id: &EntityId) -> Option<Url> {
    let Some(session_id) = manager.session_id(terminal_view_id) else {
        log::warn!("Failed to get join link args for updating browser url");
        return None;
    };
    if let Ok(url) = Url::parse(&join_link(&session_id)) {
        return Some(url);
    }
    None
}

// LOCAL FORK: the agent conversation action surface (`AgentConversationActionState`,
// `agent_conversation_action_state`, `terminal_view_for_owner_in_group`,
// `pane_group_and_terminal_view_for_owner`, `stop_local_agent_conversation`,
// `cancel_cloud_agent_task`, `stop_agent_conversation`,
// `pane_group_hosting_split_off_child`, `discard_child_agent_pane_for_conversation` and
// `kill_agent_conversation`) all went with the agent.

/// Attaches a terminal view to the pane group by subscribing to its events
/// and setting the file tree code model.
fn attach_terminal_view(
    terminal_view: &ViewHandle<TerminalView>,
    terminal_pane_id: TerminalPaneId,
    ctx: &mut ViewContext<PaneGroup>,
) {
    ctx.subscribe_to_view(
        terminal_view,
        move |group: &mut PaneGroup, _, event, ctx| {
            handle_terminal_view_event(group, terminal_pane_id, event, ctx);
        },
    );
}

/// Handles events from the pane stack when views are added or removed.
fn handle_pane_stack_event(
    group: &mut PaneGroup,
    event: &PaneStackEvent<TerminalView>,
    terminal_pane_id: TerminalPaneId,
    ctx: &mut ViewContext<PaneGroup>,
) {
    match event {
        PaneStackEvent::ViewAdded(terminal_view) => {
            attach_terminal_view(terminal_view, terminal_pane_id, ctx);
        }
        PaneStackEvent::ViewRemoved(terminal_view) => {
            ctx.unsubscribe_to_view(terminal_view);
        }
    }

    // Ensure we use the new top-level view's title and active session status.
    // TODO(ben): This shouldn't be necessary once titles are set declaratively.
    if let Some(active_terminal) = group.terminal_view_from_pane_id(terminal_pane_id, ctx) {
        active_terminal.update(ctx, |view, ctx| view.on_pane_state_change(ctx));
    }
}

fn handle_terminal_view_event(
    group: &mut PaneGroup,
    terminal_pane_id: TerminalPaneId,
    event: &Event,
    ctx: &mut ViewContext<PaneGroup>,
) {
    let pane_id = terminal_pane_id.into();

    if group.pane_contents.contains_key(&pane_id) {
        match event {
            Event::Escape => ctx.emit(pane_group::Event::Escape),
            Event::ExecuteCommand(event) => {
                ctx.emit(pane_group::Event::ExecuteCommand(event.clone()));
            }
            Event::Exited => {
                // If the shell process exited before it successfully bootstrapped,
                // keep the pane open.  There might be useful information visible
                // in the output, and if this was the first shell spawned when the
                // user started the app, it will prevent it from suddenly quitting.
                if group
                    .terminal_view_from_pane_id(terminal_pane_id, ctx)
                    .is_some_and(|terminal_view| {
                        !terminal_view.as_ref(ctx).is_login_shell_bootstrapped()
                    })
                {
                    return;
                }

                group.close_pane(pane_id, ctx);
            }
            Event::CloseRequested => {
                group.close_pane_with_confirmation(pane_id, ctx);
            }
            Event::Pane(pane_event) => group.handle_pane_event(pane_id, pane_event, ctx),
            Event::BlockListCleared => {
                // Capture CMD-K to clear blocks here so we could remove
                // all the associated blocks stored in the history.
                if let Some(terminal_pane) = group.terminal_session_by_id(pane_id) {
                    terminal_pane.delete_blocks(ctx);
                }
            }
            Event::ShareModalOpened(block_id) => {
                group.terminal_with_open_share_block_modal = Some(terminal_pane_id);
                group.share_block_modal.update(ctx, |share_modal, ctx| {
                    if let Some(session) = group.terminal_view_from_pane_id(pane_id, ctx) {
                        let model = session.read(ctx, |view, _| view.model.clone());
                        share_modal.open_with_model_update(model, *block_id, ctx);
                        ctx.notify();
                    }
                });
                ctx.notify();
            }
            Event::SendNotification(notification) => {
                ctx.emit(pane_group::Event::SendNotification {
                    notification: notification.clone(),
                    pane_id,
                })
            }
            Event::PluggableNotification { title, body } => {
                let message = if let Some(t) = title {
                    format!("{t}: {body}")
                } else {
                    body.clone()
                };
                ctx.emit(pane_group::Event::ShowToast {
                    message,
                    flavor: ToastFlavor::Default,
                    pane_id: Some(pane_id),
                })
            }
            Event::AppStateChanged => {
                ctx.emit(pane_group::Event::AppStateChanged);
            }
            Event::BlockCompleted { block, is_local } => {
                match group.terminal_session_by_id(pane_id) {
                    Some(pane) => {
                        if *GeneralSettings::as_ref(ctx).restore_session
                            && AppExecutionMode::as_ref(ctx).can_save_session()
                            && let Some(sender) = &group.model_event_sender
                        {
                            let block_completed_event = ModelEvent::SaveBlock(BlockCompleted {
                                pane_id: pane.session_uuid(),
                                block: block.clone(),
                                is_local: *is_local,
                            });

                            let sender_clone = sender.clone();
                            let _ = ctx.spawn(
                                async move {
                                    // Sending over a sync sender can block the current thread, so we do this async.
                                    sender_clone.send(block_completed_event)
                                },
                                move |_, res, _| {
                                    if let Err(err) = res {
                                        report_error!(
                                            anyhow::Error::new(err)
                                                .context("Error sending block completed event"),
                                            extra: { "terminal_pane_id" => ?terminal_pane_id }
                                        );
                                    }
                                },
                            );
                        }
                        ctx.emit(pane_group::Event::ActiveSessionChanged);
                    }
                    None => {
                        report_error!(
                            "Could not find uuid for terminal id",
                            extra: { "terminal_pane_id" => ?terminal_pane_id }
                        );
                    }
                };
            }
            Event::SessionBootstrapped => {
                ctx.emit(pane_group::Event::ActiveSessionChanged);
            }
            Event::OpenSettings(section) => {
                ctx.emit(pane_group::Event::OpenSettings(*section));
            }
            // LOCAL FORK: OpenPluginInstructionsPane and AskAIAssistant went with the agent,
            // OpenAutoReloadModal with the buy-credits banner.
            Event::SyncInput(sync_event) => {
                if SyncedInputState::as_ref(ctx)
                    .should_sync_this_pane_group(ctx.view_id(), ctx.window_id())
                {
                    ctx.emit(pane_group::Event::SyncInput(sync_event.clone()));
                }
            }
            Event::ShowCommandSearch(options) => {
                ctx.emit(pane_group::Event::ShowCommandSearch(options.clone()));
            }
            Event::TerminalViewStateChanged => {
                ctx.emit(pane_group::Event::TerminalViewStateChanged);
            }
            Event::OnboardingTutorialCompleted => {
                ctx.emit(pane_group::Event::OnboardingTutorialCompleted);
            }
            Event::OpenWorkflowModalWithCommand(command) => {
                ctx.emit(pane_group::Event::OpenWorkflowModalWithCommand(
                    command.clone(),
                ));
            }
            Event::OpenWorkflowModalWithCloudWorkflow(workflow_id) => {
                ctx.emit(pane_group::Event::OpenCloudWorkflowForEdit(*workflow_id));
            }
            Event::OpenWorkflowModalWithTemporary(workflow) => {
                ctx.emit(pane_group::Event::OpenWorkflowModalWithTemporary(
                    workflow.clone(),
                ));
            }
            Event::OpenPromptEditor => {
                ctx.emit(pane_group::Event::OpenPromptEditor);
            }
            Event::OpenAgentToolbarEditor => {
                ctx.emit(pane_group::Event::OpenAgentToolbarEditor);
            }
            Event::OpenCLIAgentToolbarEditor => {
                ctx.emit(pane_group::Event::OpenCLIAgentToolbarEditor);
            }
            Event::OpenFileInWarp { path, session } => {
                ctx.emit(pane_group::Event::OpenFileInWarp {
                    path: LocalOrRemotePath::Local(path.clone()),
                    session: session.clone(),
                });
            }
            #[cfg(feature = "local_fs")]
            Event::PreviewCodeInWarp { source } => {
                ctx.emit(pane_group::Event::PreviewCodeInWarp {
                    source: source.clone(),
                });
            }
            #[cfg(feature = "local_fs")]
            Event::OpenCodeInWarp { source, layout } => {
                ctx.emit(pane_group::Event::OpenCodeInWarp {
                    source: source.clone(),
                    layout: *layout,
                    line_col: if let CodeSource::Link { range_start, .. } = source {
                        *range_start
                    } else {
                        None
                    },
                });
            }
            // LOCAL FORK: OpenCodeDiff went with the agent.
            Event::OpenCodeReviewPane(arg) => {
                ctx.emit(pane_group::Event::OpenCodeReviewPane(arg.clone()));
            }
            Event::OpenCodeReviewPaneAndScrollToComment {
                open_code_review,
                comment,
                diff_mode,
            } => {
                ctx.emit(pane_group::Event::OpenCodeReviewPaneAndScrollToComment {
                    open_code_review: open_code_review.clone(),
                    comment: comment.clone(),
                    diff_mode: diff_mode.clone(),
                });
            }
            Event::ImportAllCodeReviewComments {
                open_code_review,
                comments,
                diff_mode,
            } => {
                ctx.emit(pane_group::Event::ImportAllCodeReviewComments {
                    open_code_review: open_code_review.clone(),
                    comments: comments.clone(),
                    diff_mode: diff_mode.clone(),
                });
            }
            Event::ToggleCodeReviewPane(arg) => {
                ctx.emit(pane_group::Event::ToggleCodeReviewPane(arg.clone()));
            }
            Event::OpenShareSessionModal { open_source } => {
                group.open_share_session_modal(terminal_pane_id, *open_source, ctx)
            }
            // When the host's manual share stops, also stop the share on
            // any local children whose share was auto-created via
            // `inherit_share_for_local_child`. Skipped on wasm because the
            // transitive-share tracker is only populated on non-wasm
            // dispatch paths.
            #[cfg(not(target_family = "wasm"))]
            Event::StopSharingCurrentSession { .. } => {
                group.stop_transitively_shared_child_shares(pane_id, ctx);
            }
            Event::OpenShareSessionDeniedModal => {
                group.open_share_session_denied_modal(terminal_pane_id, ctx);
            }
            Event::FocusSession => {
                group.focus_pane(terminal_pane_id.into(), true, ctx);
                ctx.emit(pane_group::Event::FocusPaneGroup);
            }
            Event::OpenSharedSessionRoleChangeModal { source } => match source {
                RoleChangeOpenSource::ViewerRequest { role } => {
                    group.open_shared_session_viewer_request_modal(terminal_pane_id, *role, ctx)
                }
                RoleChangeOpenSource::SharerResponse {
                    participant_id,
                    role_request_id,
                    role,
                } => group.open_shared_session_sharer_response_modal(
                    terminal_pane_id,
                    participant_id.clone(),
                    role_request_id.clone(),
                    *role,
                    ctx,
                ),
                RoleChangeOpenSource::SharerGrant { participant_id } => group
                    .open_shared_session_sharer_grant_modal(
                        terminal_pane_id,
                        participant_id.clone(),
                        ctx,
                    ),
            },
            Event::CloseSharedSessionRoleChangeModal(source) => {
                group.close_shared_session_role_change_modal(*source, ctx);
            }
            Event::RoleRequestInFlight { role_request_id } => {
                group.set_shared_session_role_change_modal_request_id(role_request_id.clone(), ctx);
            }
            Event::RoleRequestCancelled(role_request_id) => {
                group.remove_shared_session_role_request(role_request_id.clone(), ctx);
            }
            Event::OpenWarpDriveObjectInPane(uid) => {
                ctx.emit(pane_group::Event::OpenWarpDriveObjectInPane(uid.clone()));
            }
            // LOCAL FORK: OpenSuggestedAgentModeWorkflowModal and OpenSuggestedRuleDialog
            // went with the agent.
            Event::OpenAIFactCollection { sync_id } => {
                ctx.emit(pane_group::Event::OpenAIFactCollection { sync_id: *sync_id });
            }
            Event::SummarizationCancelDialogToggled { is_open } => {
                group.terminal_with_open_summarization_dialog = is_open.then_some(terminal_pane_id);
                ctx.notify();
            }
            Event::EnvironmentSetupModeSelectorToggled { is_open } => {
                group.pane_with_open_environment_setup_mode_selector = is_open.then_some(pane_id);
                ctx.notify();
            }
            Event::AuthSecretDeleteConfirmationDialogToggled { is_open } => {
                group.pane_with_open_auth_secret_delete_confirmation_dialog =
                    is_open.then_some(pane_id);
                ctx.notify();
            }
            #[cfg(feature = "local_fs")]
            Event::OpenFileWithTarget {
                path,
                target,
                line_col,
            } => {
                ctx.emit(pane_group::Event::OpenFileWithTarget {
                    path: path.clone(),
                    target: target.clone(),
                    line_col: *line_col,
                });
            }
            Event::CopyFileToRemote { command, upload_id } => {
                let new_pane_id = group.insert_terminal_pane(
                    Direction::Right,
                    pane_id,
                    None, /*chosen_shell*/
                    ctx,
                );

                group.hide_pane_for_job(new_pane_id.into(), ctx);

                let new_terminal_view = group
                    .active_session_view(ctx)
                    .expect("should have new terminal view");
                new_terminal_view.update(ctx, |terminal_view, ctx| {
                    terminal_view.set_pending_command(command, ctx);
                    terminal_view.set_is_ssh_uploader(true);
                });

                ctx.emit(pane_group::Event::FileUploadCommand {
                    upload_id: *upload_id,
                    command: command.to_owned(),
                    remote_pane_id: terminal_pane_id,
                    local_pane_id: new_pane_id,
                });

                group.focus_pane(pane_id, true, ctx);
            }
            Event::FileUploadPasswordPending => {
                ctx.emit(pane_group::Event::FileUploadPasswordPending {
                    local_pane_id: terminal_pane_id,
                });
            }
            Event::OpenConversationHistory => {
                ctx.emit(OpenConversationHistory);
            }
            Event::FileUploadFinished(exit_code) => {
                ctx.emit(pane_group::Event::FileUploadFinished {
                    local_pane_id: terminal_pane_id,
                    exit_code: *exit_code,
                });

                // Each upload spawns its own new terminal pane. Once an upload
                // has finished, we know that its terminal session will no
                // longer be responsible for any UI-based uploads.
                if let Some(uploader_terminal_view) =
                    group.terminal_view_from_pane_id(terminal_pane_id, ctx)
                {
                    uploader_terminal_view.update(ctx, |terminal_view, _ctx| {
                        terminal_view.set_is_ssh_uploader(false);
                    });
                }
            }
            Event::OpenFileUploadSession(upload_id) => {
                ctx.emit(pane_group::Event::OpenFileUploadSession {
                    remote_pane_id: terminal_pane_id,
                    upload_id: *upload_id,
                })
            }
            Event::TerminateFileUploadSession(upload_id) => {
                ctx.emit(pane_group::Event::TerminateFileUploadSession {
                    remote_pane_id: terminal_pane_id,
                    upload_id: *upload_id,
                })
            }
            Event::OpenThemeChooser => {
                ctx.emit(pane_group::Event::OpenThemeChooser);
            }
            // LOCAL FORK: OpenMCPSettingsPage went with the agent.
            Event::OpenFilesPalette { source } => {
                ctx.emit(pane_group::Event::OpenFilesPalette { source: *source })
            }
            Event::OpenAddRulePane => {
                ctx.emit(crate::pane_group::Event::OpenAddRulePane);
            }
            Event::OpenRulesPane => {
                ctx.emit(crate::pane_group::Event::OpenAIFactCollection { sync_id: None });
            }
            Event::OpenAddPromptPane { initial_content } => {
                ctx.emit(crate::pane_group::Event::OpenAddPromptPane {
                    initial_content: initial_content.clone(),
                });
            }
            // LOCAL FORK: the OpenEnvironmentManagementPane forward went with the cloud
            // Environments settings page.
            #[cfg(feature = "local_fs")]
            Event::FileRenamed { old_path, new_path } => {
                ctx.emit(pane_group::Event::FileRenamed {
                    old_path: old_path.clone(),
                    new_path: new_path.clone(),
                });
            }
            #[cfg(feature = "local_fs")]
            Event::FileDeleted { path } => {
                ctx.emit(pane_group::Event::FileDeleted { path: path.clone() });
            }
            Event::ToggleLeftPanel {
                target_view,
                force_open,
            } => {
                ctx.emit(pane_group::Event::ToggleLeftPanel {
                    target_view: *target_view,
                    force_open: *force_open,
                });
            }
            // LOCAL FORK: the AI document panes and the agent profile editor are gone;
            // only the "hide" event still exists on the terminal view.
            Event::HideAIDocumentPanes => {}
            // LOCAL FORK: the child agent orchestration events (RevealChildAgent,
            // SwapPaneToConversation, EnsureSharedSessionViewerChildPane,
            // OpenChildAgentInNewTab, OpenChildAgentInNewPane, StopAgentConversation,
            // KillAgentConversation and StartAgentConversation) all went with the agent.
            _ => {}
        }
    } else {
        log::warn!("Session {terminal_pane_id:?} not found");
    }
}

// LOCAL FORK: fn dispatch_start_agent_conversation, fn launch_local_no_harness_child,
// fn launch_remote_child and fn handle_ai_history_event all went with the agent.
