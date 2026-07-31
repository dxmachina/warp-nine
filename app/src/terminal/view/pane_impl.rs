//! This module contains the implementation of `BackingView` for `TerminalView`, as well as
//! business logic for integrating the terminal view with the pane infra (`crate::pane_group`).
use settings::Setting as _;
use warp_core::context_flag::ContextFlag;
use warpui::elements::{
    ConstrainedBox, CrossAxisAlignment, Flex, MainAxisAlignment, MainAxisSize, ParentElement,
    Shrinkable,
};
use warpui::prelude::{ChildView, Container};
use warpui::text_layout::ClipConfig;
use warpui::{
    AppContext, Element, ModelHandle, SingletonEntity, TypedActionView, ViewContext,
    WeakModelHandle,
};

use super::shared_session::adapter::Kind as SharedSessionKind;
use super::{Event, PaneConfiguration, TerminalAction, TerminalViewState, Viewer};
use crate::appearance::Appearance;
use crate::features::FeatureFlag;
use crate::menu::{MenuItem, MenuItemFields};
use crate::pane_group::focus_state::{PaneFocusHandle, PaneGroupFocusEvent, PaneGroupFocusState};
use crate::pane_group::pane::view::header::components::{
    CenteredHeaderEdgeWidth, header_edge_min_width, render_pane_header_buttons,
    render_pane_header_title_text, render_three_column_header,
};
use crate::pane_group::pane::view::header::render_pane_header_draggable;
use crate::pane_group::pane::{PaneStack, view};
use crate::pane_group::{BackingView, SplitPaneState, TOGGLE_MAXIMIZE_PANE_BINDING_NAME};
use crate::settings::app_installation_detection::{
    UserAppInstallDetectionSettings, UserAppInstallStatus,
};
use crate::sharing::ShareableObject;
use crate::terminal::shared_session::SharedSessionActionSource;
use crate::terminal::shared_session::participant_avatar_view::render_participants_and_role_elements;
use crate::terminal::shared_session::render_util::shared_session_indicator_color;
use crate::terminal::{TerminalManager, TerminalView};
use crate::ui_components::{blended_colors, icons};
use crate::util::bindings::keybinding_name_to_display_string;

// LOCAL FORK: const PANE_HEADER_AGENT_SIZE removed with the agent icon-with-status
// component it sized.

impl TerminalView {
    /// Returns a reference to the focus handle if one has been set.
    pub fn focus_handle(&self) -> Option<&PaneFocusHandle> {
        self.focus_handle.as_ref()
    }

    fn handle_focus_state_event(
        &mut self,
        _focus_state: ModelHandle<PaneGroupFocusState>,
        event: &PaneGroupFocusEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(focus_handle) = &self.focus_handle else {
            return;
        };

        if focus_handle.is_affected(event) {
            self.on_pane_state_change(ctx);
        }
    }

    /// Set the pane configuration for this terminal view.
    pub fn set_pane_configuration(&mut self, pane_configuration: ModelHandle<PaneConfiguration>) {
        self.pane_configuration = pane_configuration;
    }

    /// Respond to changes to the active session or split pane states.
    pub fn on_pane_state_change(&mut self, ctx: &mut ViewContext<Self>) {
        self.refresh_pane_header(ctx);

        // Trigger refresh of the pane header overflow menu to reflect the new pane state
        // (e.g., updating the Maximize/Minimize pane menu item)
        self.pane_configuration.update(ctx, |config, ctx| {
            config.refresh_pane_header_overflow_menu_items(ctx);
        });

        if !self.is_pane_focused(ctx) {
            // Don't need to call ctx.notify here as clear_selected_blocks already
            // calls ctx.notify internally
            self.clear_selected_blocks(ctx);
            self.clear_selected_text(ctx);
        } else {
            ctx.notify();
        }
    }

    pub fn refresh_pane_header(&mut self, ctx: &mut ViewContext<Self>) {
        let is_active_session = self.is_active_session(ctx);
        self.pane_configuration
            .update(ctx, move |pane_config, ctx| {
                pane_config.set_show_active_pane_indicator(is_active_session, ctx);
                pane_config.refresh_pane_header_overflow_menu_items(ctx);
            });
    }

    /// Set the pane title from the terminal title.
    ///
    /// LOCAL FORK: CLI agent titles and conversation titles went with the agent, so
    /// the terminal title is the only remaining source.
    pub(super) fn update_pane_configuration(&mut self, ctx: &mut ViewContext<Self>) {
        self.is_using_conversation_for_pane_header_title = false;
        let new_pane_title = self.terminal_title.clone();
        self.pane_configuration.update(ctx, |pane_config, ctx| {
            pane_config.set_title(new_pane_title, ctx);
            if FeatureFlag::AgentView.is_enabled() {
                pane_config.refresh_pane_header_overflow_menu_items(ctx);
            }
            pane_config.notify_header_content_changed(ctx);
        });
        self.update_agent_view_pane_header(ctx);
    }

    /// Returns the shareable object for this pane, if any.
    ///
    /// LOCAL FORK: the AI-conversation shareable object went with the agent; only the
    /// shared-session one is left.
    fn agent_view_shareable_object(&self, ctx: &ViewContext<Self>) -> Option<ShareableObject> {
        // Only set shareable object if CloudConversations feature is enabled
        if !FeatureFlag::CloudConversations.is_enabled() {
            return None;
        }

        let shared_session = self.shared_session.as_ref()?;
        Some(ShareableObject::Session {
            handle: ctx.handle(),
            session_id: *shared_session.session_id(),
            started_at: *shared_session.started_at(),
        })
    }

    /// Updates the pane header's shareable object.
    /// This should be called when the shared session changes.
    pub(super) fn update_agent_view_pane_header(&mut self, ctx: &mut ViewContext<Self>) {
        if !FeatureFlag::AgentView.is_enabled() {
            return;
        }

        let shareable_object = self.agent_view_shareable_object(ctx);
        self.pane_configuration.update(ctx, |pane_config, ctx| {
            pane_config.set_shareable_object(shareable_object, ctx);
            pane_config.notify_header_content_changed(ctx);
            pane_config.refresh_pane_header_overflow_menu_items(ctx);
        });
    }

    pub(super) fn is_pane_focused(&self, app: &AppContext) -> bool {
        self.focus_handle.as_ref().is_none_or(|h| h.is_focused(app))
    }

    pub fn is_active_session(&self, app: &AppContext) -> bool {
        self.focus_handle
            .as_ref()
            .is_some_and(|h| h.is_active_session(app))
    }

    pub(super) fn split_pane_state(&self, app: &AppContext) -> SplitPaneState {
        self.focus_handle
            .as_ref()
            .map_or(SplitPaneState::NotInSplitPane, |h| h.split_pane_state(app))
    }

    // LOCAL FORK: fn maybe_render_header_back_button removed with the agent — the
    // pane header back button only ever navigated the agent view stack.

    fn render_header_title(
        &self,
        header_ctx: &view::HeaderRenderContext,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let pane_config = self.pane_configuration.as_ref(app);
        let title = pane_config.title().to_owned();
        let clip_config = if self.is_using_conversation_for_pane_header_title {
            ClipConfig::ellipsis()
        } else {
            ClipConfig::start()
        };

        let should_render_ambient_agent_indicator = self.is_cloud_agent_session(app);
        let pane_indicator = if should_render_ambient_agent_indicator {
            // LOCAL FORK: no agent means no agent status circle to render here.
            None
        } else if let Some(shared_session) = self.shared_session.as_ref() {
            if let Some(Viewer {
                sharer: Some(sharer),
                ..
            }) = shared_session.kind().as_viewer()
            {
                Some(
                    Container::new(ChildView::new(&sharer.avatar).finish())
                        .with_margin_right(4.)
                        .finish(),
                )
            } else {
                Some(
                    ConstrainedBox::new(
                        icons::Icon::Sharing
                            .to_warpui_icon(shared_session_indicator_color(appearance).into())
                            .finish(),
                    )
                    .with_height(appearance.ui_font_size())
                    .with_width(appearance.ui_font_size())
                    .finish(),
                )
            }
        } else {
            // LOCAL FORK: the agent-status branch that keyed off the AI context model
            // went with the agent.
            self.render_terminal_mode_indicator(app)
        };

        let is_pane_dragging = header_ctx.draggable_state.is_dragging();
        let mut center_row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Min);
        if let Some(indicator) = pane_indicator {
            center_row.add_child(Container::new(indicator).with_margin_right(4.).finish());
        }
        let title_text = render_pane_header_title_text(title, appearance, clip_config);
        if is_pane_dragging {
            // During drag, all children must be non-flex to avoid panics
            // from infinite constraints on flex children.
            center_row.add_child(title_text);
        } else {
            // LOCAL FORK: the width-capped fullscreen-agent-view title went with the agent.
            center_row.add_child(Shrinkable::new(1.0, title_text).finish());
        }

        center_row.finish()
    }

    /// Returns the right-column element and the estimated minimum width of
    /// the right-column content (used to set the edge width for centering).
    fn render_header_actions(
        &self,
        header_ctx: &view::HeaderRenderContext,
        app: &AppContext,
    ) -> (Box<dyn Element>, f32) {
        let appearance = Appearance::as_ref(app);
        let icon_color = Some(
            appearance
                .theme()
                .sub_text_color(appearance.theme().background()),
        );
        // LOCAL FORK: the fullscreen agent view sized these buttons down; without it
        // they always use the default size.
        let button_size: Option<f32> = None;

        let left_of_overflow = self.render_shared_session_header_content(app);

        // LOCAL FORK: the ambient agent cancel button and the conversation details
        // toggle went with the agent.
        let mut icon_button_count: u32 = 0;

        let mut right_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Min);
        if let Some(content) = left_of_overflow {
            right_row.add_child(content);
        }
        let sharing_element = header_ctx.sharing_controls(app, icon_color, button_size);
        let has_sharing_element = sharing_element.is_some();
        if let Some(sharing) = sharing_element {
            right_row.add_child(sharing);
        }
        let show_close_button = self
            .focus_handle
            .as_ref()
            .is_some_and(|h| h.is_in_split_pane(app));
        right_row.add_child(
            render_pane_header_buttons::<TerminalAction, TerminalAction>(
                header_ctx,
                appearance,
                show_close_button,
                icon_color,
                button_size,
            ),
        );
        icon_button_count += show_close_button as u32
            + header_ctx.has_overflow_items as u32
            + has_sharing_element as u32;

        let min_width = header_edge_min_width(icon_button_count);
        (right_row.finish(), min_width)
    }

    // LOCAL FORK: fn maybe_add_parent_navigation_card removed with the agent — the
    // orchestration pill bar and the parent conversation card were both agent chrome.

    fn render_terminal_pane_header(
        &self,
        header_ctx: &view::HeaderRenderContext,
        app: &AppContext,
    ) -> Box<dyn Element> {
        // LOCAL FORK: the agent view's back button occupied the left column.
        let left = Flex::row().finish();
        let center = self.render_header_title(header_ctx, app);
        let (right, min_actions_width) = self.render_header_actions(header_ctx, app);

        let header = render_three_column_header(
            left,
            center,
            right,
            CenteredHeaderEdgeWidth {
                min: min_actions_width,
                max: 200.0,
            },
            header_ctx.header_left_inset,
            header_ctx.draggable_state.is_dragging(),
        );
        render_pane_header_draggable::<TerminalView>(
            self.pane_configuration.clone(),
            header,
            header_ctx.draggable_state.clone(),
            app,
        )
    }
}

impl BackingView for TerminalView {
    type PaneHeaderOverflowMenuAction = TerminalAction;
    type CustomAction = TerminalAction;
    type AssociatedData = ModelHandle<Box<dyn TerminalManager>>;

    fn set_pane_stack(
        &mut self,
        pane_stack: WeakModelHandle<PaneStack<Self>>,
        _ctx: &mut ViewContext<Self>,
    ) {
        self.pane_stack = Some(pane_stack);
    }

    fn handle_pane_header_overflow_menu_action(
        &mut self,
        action: &Self::PaneHeaderOverflowMenuAction,
        ctx: &mut ViewContext<Self>,
    ) {
        self.handle_action(action, ctx);
    }

    fn handle_custom_action(&mut self, action: &Self::CustomAction, ctx: &mut ViewContext<Self>) {
        self.handle_action(action, ctx);
    }

    fn close(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.emit(Event::CloseRequested);
    }

    fn focus_contents(&mut self, ctx: &mut ViewContext<Self>) {
        self.redetermine_global_focus(ctx);
    }

    fn on_pane_header_overflow_menu_toggled(&mut self, is_open: bool, ctx: &mut ViewContext<Self>) {
        self.pane_header_overflow_menu_toggled(is_open, ctx);
    }

    fn pane_header_overflow_menu_items(
        &self,
        ctx: &AppContext,
    ) -> Vec<MenuItem<Self::PaneHeaderOverflowMenuAction>> {
        let model = self.model.lock();
        let mut items = vec![];
        let source = SharedSessionActionSource::PaneHeader;

        // Shared-session related items.
        let shared_session_status = model.shared_session_status();
        let is_ambient_agent = self.is_ambient_agent_session(ctx);
        if shared_session_status.is_sharer_or_viewer() {
            if !is_ambient_agent {
                items.push(
                    MenuItemFields::new("Copy link")
                        .with_on_select_action(TerminalAction::CopySharedSessionLink { source })
                        .into_item(),
                );
            }

            if shared_session_status.is_sharer() {
                items.push(
                    MenuItemFields::new("Stop sharing session")
                        .with_on_select_action(TerminalAction::StopSharingCurrentSession { source })
                        .into_item(),
                );
            }
            if !ContextFlag::HideOpenOnDesktopButton.is_enabled()
                && *UserAppInstallDetectionSettings::as_ref(ctx)
                    .user_app_installation_detected
                    .value()
                    == UserAppInstallStatus::Detected
            {
                items.push(
                    MenuItemFields::new("Open on Desktop")
                        .with_on_select_action(TerminalAction::OpenSharedSessionOnDesktop {
                            source,
                        })
                        .into_item(),
                );
            }
        } else if FeatureFlag::CreatingSharedSessions.is_enabled()
            && ContextFlag::CreateSharedSession.is_enabled()
        {
            items.push(
                MenuItemFields::new("Share session")
                    .with_on_select_action(TerminalAction::OpenShareSessionModal { source })
                    .into_item(),
            );
        }

        // Split-pane related items.
        if self.split_pane_state(ctx).is_in_split_pane() {
            if !items.is_empty() {
                items.push(MenuItem::Separator);
            }

            let is_maximized = self.split_pane_state(ctx).is_maximized();
            items.push(
                MenuItemFields::toggle_pane_action(is_maximized)
                    .with_on_select_action(TerminalAction::ToggleMaximizePane)
                    .with_key_shortcut_label(keybinding_name_to_display_string(
                        TOGGLE_MAXIMIZE_PANE_BINDING_NAME,
                        ctx,
                    ))
                    .into_item(),
            );
        }

        items
    }

    fn should_render_header(&self, app: &AppContext) -> bool {
        let is_shared = self
            .model
            .lock()
            .shared_session_status()
            .is_sharer_or_viewer();
        // LOCAL FORK: the fullscreen agent view also forced the header on.
        is_shared
            || FeatureFlag::ContextWindowUsageV2.is_enabled()
                && self.split_pane_state(app).is_in_split_pane()
    }

    fn render_header_content(
        &self,
        header_ctx: &view::HeaderRenderContext<'_>,
        app: &AppContext,
    ) -> view::HeaderContent {
        view::HeaderContent::Custom {
            element: self.render_terminal_pane_header(header_ctx, app),
            // We wrap only the title row in the drag handler ourselves;
            // the secondary row stays interactive.
            has_custom_draggable_behavior: true,
        }
    }

    /// Sets the focus handle for this terminal view, enabling it to track its split pane state.
    fn set_focus_handle(&mut self, focus_handle: PaneFocusHandle, ctx: &mut ViewContext<Self>) {
        self.focus_handle = Some(focus_handle.clone());
        // Subscribe to focus state changes to update pane state when focus/split state changes
        ctx.subscribe_to_model(
            focus_handle.focus_state_handle(),
            Self::handle_focus_state_event,
        );
        self.input.update(ctx, |input, ctx| {
            input.set_focus_handle(focus_handle, ctx);
        });
        self.on_pane_state_change(ctx);
    }
}

impl TerminalView {
    // LOCAL FORK: fns render_ambient_agent_cancel_button and
    // render_conversation_details_toggle_button removed with the agent.

    /// Render the indicator for terminal mode (no conversation selected).
    /// Shows error indicator if terminal is in error state, otherwise shell indicator on Windows.
    fn render_terminal_mode_indicator(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let appearance = Appearance::as_ref(app);
        let font_size = appearance.ui_font_size();

        // Error indicator takes priority
        if matches!(self.current_state.state, TerminalViewState::Errored) {
            return Some(
                ConstrainedBox::new(
                    icons::Icon::AlertTriangle
                        .to_warpui_icon(appearance.theme().ui_error_color().into())
                        .finish(),
                )
                .with_height(font_size)
                .with_width(font_size)
                .finish(),
            );
        }

        // Shell indicator (Windows only)
        if let Some(shell_indicator_type) = self.shell_indicator_type {
            let shell_indicator_icon = shell_indicator_type
                .to_icon()
                .to_warpui_icon(
                    blended_colors::text_sub(appearance.theme(), appearance.theme().background())
                        .into(),
                )
                .finish();
            return Some(
                ConstrainedBox::new(shell_indicator_icon)
                    .with_height(font_size)
                    .with_width(font_size)
                    .finish(),
            );
        }

        None
    }

    /// Render shared session header content (participant avatars and role controls).
    fn render_shared_session_header_content(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let Some(shared_session) = &self.shared_session else {
            return None;
        };

        let presence_manager = shared_session.presence_manager();
        let role = presence_manager.as_ref(app).role();

        // Get viewer avatars to render
        let viewers = shared_session.pane_header_viewer_avatars(app);

        // Get role change menu info based on session kind
        let (role_change_menu, is_role_change_menu_open, mouse_state_handle) =
            match shared_session.kind() {
                SharedSessionKind::Viewer(viewer) => (
                    Some(viewer.role_change_menu.clone()),
                    viewer.is_role_change_menu_open,
                    viewer.role_change_menu_button.clone(),
                ),
                SharedSessionKind::Sharer(sharer) => {
                    (None, false, sharer.revoke_all_mouse_state_handle().clone())
                }
            };

        // Hide role change button in cloud mode conversations
        let hide_role_change_button = self.model.lock().is_shared_ambient_agent_session();

        // Render participant avatars and role elements
        Some(render_participants_and_role_elements(
            viewers,
            role,
            mouse_state_handle,
            role_change_menu,
            is_role_change_menu_open,
            hide_role_change_button,
            app,
        ))
    }

    /// LOCAL FORK: the ambient agent view model went with the agent, so a pane is never
    /// an ambient agent session. Kept because several kept surfaces still ask.
    pub fn is_ambient_agent_session(&self, _ctx: &AppContext) -> bool {
        false
    }

    /// Whether this pane should be treated as an ambient agent conversation for display
    /// purposes (e.g. the ambient agent icon in the pane header and vertical tab). This is the
    /// single source of truth for that check; surfaces should call it rather than re-deriving
    /// the condition, so they can't drift apart.
    ///
    /// It deliberately does NOT treat a manually shared *local* (`User`) session as a cloud
    /// agent session even though it now carries an orchestrator task id on its `source_task_id`
    /// sidecar (see QUALITY-726).
    pub fn is_cloud_agent_session(&self, ctx: &AppContext) -> bool {
        self.is_ambient_agent_session(ctx) || self.model.lock().is_cloud_agent_conversation()
    }

    /// LOCAL FORK: agent conversations went with the agent, so there is never a
    /// conversation title to show. Kept because the vertical tabs still ask.
    pub fn selected_conversation_display_title(&self, _ctx: &AppContext) -> Option<String> {
        None
    }

    // LOCAL FORK: fns selected_conversation_is_empty and
    // selected_conversation_is_local_child removed with the agent.
}
