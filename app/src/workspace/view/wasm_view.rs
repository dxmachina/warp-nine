//! WASM-only view functions for the Workspace.

use warp_core::channel::ChannelState;
use warpui::elements::{ChildView, Element};
use warpui::{AppContext, SingletonEntity, ViewContext, ViewHandle};

use super::PanelPosition;
use crate::BlocklistAIHistoryModel;
use crate::ai::conversation_details_panel::{
    ConversationDetailsData, ConversationDetailsPanel, ConversationDetailsPanelEvent,
};
use crate::terminal::TerminalView;
use crate::ui_components::icons;
use crate::uri::browser_url_handler::parse_current_url;
use crate::view_components::action_button::{
    ActionButton, ButtonSize, NakedTheme, PrimaryTheme, SecondaryTheme,
};
use crate::wasm_nux_dialog::{WasmNUXDialog, WasmNUXDialogEvent};
use crate::workspace::action::WorkspaceAction;
use crate::workspace::view::{NotebookSource, OpenWarpDriveObjectSettings, Workspace};

const TRANSCRIPT_PANEL_WIDTH: f32 = 280.0;

/// Builds the OZ runs URL for viewing all cloud runs.
fn build_oz_runs_url() -> String {
    format!("{}/runs", ChannelState::oz_root_url())
}

impl Workspace {
    pub(super) fn build_wasm_nux_dialog(ctx: &mut ViewContext<Self>) -> ViewHandle<WasmNUXDialog> {
        let wasm_nux_dialog = ctx.add_typed_action_view(|_| WasmNUXDialog::new());
        ctx.subscribe_to_view(&wasm_nux_dialog, |me, _, event, ctx| match event {
            WasmNUXDialogEvent::Close => {
                me.show_wasm_nux_dialog = false;
                ctx.notify();
            }
        });
        wasm_nux_dialog
    }

    pub(super) fn build_open_in_warp_button(
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<ActionButton> {
        ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Open in Warp", PrimaryTheme).on_click(move |ctx| {
                // Get the current URL and dispatch action to open it on desktop
                if let Some(url) = parse_current_url() {
                    ctx.dispatch_typed_action(WorkspaceAction::OpenLinkOnDesktop(url));
                } else {
                    log::warn!("Could not get URL for Open in Warp button");
                }
            })
        })
    }

    pub(super) fn build_view_cloud_runs_button(
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<ActionButton> {
        let url = build_oz_runs_url();
        ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("View all cloud runs", SecondaryTheme).on_click(move |ctx| {
                ctx.dispatch_typed_action(WorkspaceAction::OpenLink(url.clone()));
            })
        })
    }

    pub(super) fn build_transcript_info_button(
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<ActionButton> {
        ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", NakedTheme)
                .with_icon(icons::Icon::Info)
                .with_size(ButtonSize::Small)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(
                        WorkspaceAction::ToggleConversationTranscriptDetailsPanel,
                    );
                })
        })
    }

    pub(super) fn build_transcript_details_panel(
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<ConversationDetailsPanel> {
        let panel = ctx.add_typed_action_view(|ctx| {
            ConversationDetailsPanel::new(false, TRANSCRIPT_PANEL_WIDTH, ctx)
        });

        ctx.subscribe_to_view(&panel, |me, _, event, ctx| match event {
            ConversationDetailsPanelEvent::Close => {
                me.current_workspace_state.is_transcript_details_panel_open = false;
                me.transcript_info_button.update(ctx, |button, ctx| {
                    button.set_active(false, ctx);
                });
                ctx.notify();
            }
            ConversationDetailsPanelEvent::OpenPlanNotebook { notebook_uid } => {
                me.open_notebook(
                    &NotebookSource::Existing((*notebook_uid).into()),
                    &OpenWarpDriveObjectSettings::default(),
                    ctx,
                    true,
                );
            }
        });

        panel
    }

    /// Check if we should show the conversation details panel, given the focused terminal view.
    /// Returns true for:
    /// - Conversation transcript viewers (always)
    /// - Shared sessions with an active conversation
    ///
    /// LOCAL FORK: the restored-ambient-cloud-task case went with the agent.
    pub(super) fn should_show_conversation_details_panel(
        focused_terminal_view: &ViewHandle<TerminalView>,
        ctx: &AppContext,
    ) -> bool {
        let terminal_view_ref = focused_terminal_view.as_ref(ctx);
        let model = terminal_view_ref.model.lock();

        // Always show for conversation transcript viewers
        if model.is_conversation_transcript_viewer() {
            return true;
        }
        // For shared sessions, show if there's an active conversation.
        if model.shared_session_status().is_sharer_or_viewer() {
            drop(model); // Release lock before accessing BlocklistAIHistoryModel
            return BlocklistAIHistoryModel::as_ref(ctx)
                .active_conversation(focused_terminal_view.id())
                .is_some();
        }

        false
    }

    /// Renders the transcript details panel for WASM conversation transcript and shared session viewing.
    pub(super) fn render_transcript_details_panel(
        &self,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        let terminal_view = self
            .active_tab_pane_group()
            .as_ref(app)
            .focused_session_view(app)?;

        if !Self::should_show_conversation_details_panel(&terminal_view, app) {
            return None;
        }

        Some(self.render_panel(
            app,
            ChildView::new(&self.transcript_details_panel).finish(),
            &PanelPosition::Right,
        ))
    }

    pub(super) fn update_transcript_details_panel_data(&mut self, ctx: &mut ViewContext<Self>) {
        // Get the focused terminal view
        let Some(terminal_view) = self
            .active_tab_pane_group()
            .as_ref(ctx)
            .focused_session_view(ctx)
        else {
            return;
        };

        if !Self::should_show_conversation_details_panel(&terminal_view, ctx) {
            return;
        }

        let terminal_view_id = terminal_view.id();

        self.transcript_details_panel.update(ctx, |panel, ctx| {
            // LOCAL FORK: the ambient agent task branch, which populated the panel from
            // cloud task data, went with the agent.
            let history_model = BlocklistAIHistoryModel::handle(ctx).as_ref(ctx);
            if let Some(conversation) = history_model.active_conversation(terminal_view_id) {
                let details = ConversationDetailsData::from_conversation(conversation, ctx);
                panel.set_conversation_details(details, ctx);
            }
            ctx.notify();
        });
    }
}
