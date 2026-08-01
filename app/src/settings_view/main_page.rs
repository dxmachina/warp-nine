use std::sync::{Arc, Mutex};

use ::settings::{Setting, ToggleableSetting};
use lazy_static::lazy_static;
use pathfinder_color::ColorU;
use warp_core::channel::ChannelState;
use warp_core::context_flag::ContextFlag;
use warp_errors::{report_error, report_if_error};
#[cfg(not(target_family = "wasm"))]
use warp_server_client::iap::{IapCredentialsState, IapManager, IapManagerEvent};
use warpui::elements::{
    Align, Border, Container, CrossAxisAlignment, Element, Empty, Flex, MouseStateHandle,
    ParentElement, Shrinkable, Text,
};
use warpui::keymap::ContextPredicate;
use warpui::platform::Cursor;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::ui_components::switch::SwitchStateHandle;
use warpui::{
    Action, AppContext, Entity, ModelHandle, SingletonEntity, TypedActionView, View, ViewContext,
    ViewHandle, WeakViewHandle, id,
};

use super::settings_page::{
    AdditionalInfo, LocalOnlyIconState, MatchData, PageType, SettingsPageMeta,
    SettingsPageViewHandle, SettingsWidget, ToggleState, render_body_item,
};
use super::{SettingsAction, SettingsSection, ToggleSettingActionPair, flags};
use crate::appearance::Appearance;
use crate::auth::AuthStateProvider;
use crate::auth::auth_state::AuthState;
use crate::autoupdate::{self, AutoupdateStage, AutoupdateState};
use crate::settings::cloud_preferences::CloudPreferencesSettings;
use crate::workspace::WorkspaceAction;
use crate::workspaces::update_manager::TeamUpdateManager;

const PHOTO_SIZE: f32 = 40.;
const REGULAR_TEXT_FONT_SIZE: f32 = 12.;
const VERTICAL_MARGIN: f32 = 24.;
lazy_static! {
    static ref SETTINGS_SYNC_BINDINGS_ADDED: Arc<Mutex<bool>> = Default::default();
}

pub fn init_actions_from_parent_view<T: Action + Clone>(
    app: &mut AppContext,
    context: &ContextPredicate,
    builder: fn(SettingsAction) -> T,
) {
    let mut toggle_binding_pairs = Vec::new();
    maybe_add_settings_sync_toggle_binding(app, context, builder, &mut toggle_binding_pairs);

    // Add other bindings here in the future.

    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(toggle_binding_pairs, app);
}

fn maybe_add_settings_sync_toggle_binding<T: Action + Clone>(
    app: &mut AppContext,
    context: &ContextPredicate,
    builder: fn(SettingsAction) -> T,
    toggle_binding_pairs: &mut Vec<ToggleSettingActionPair<T>>,
) {
    let mut lock = SETTINGS_SYNC_BINDINGS_ADDED
        .lock()
        .expect("settings sync bindings lock poisoned");
    if !*lock {
        *lock = true;
        toggle_binding_pairs.push(
            ToggleSettingActionPair::new(
                "settings sync",
                builder(SettingsAction::MainPageToggle(
                    MainPageAction::ToggleSettingsSync,
                )),
                context,
                flags::SETTINGS_SYNC_FLAG,
            )
            .is_supported_on_current_platform(
                CloudPreferencesSettings::as_ref(app)
                    .settings_sync_enabled
                    .is_supported_on_current_platform(),
            ),
        );
    }
}

pub fn handle_experiment_change(app: &mut AppContext) {
    let mut toggle_binding_pairs: Vec<ToggleSettingActionPair<WorkspaceAction>> = Vec::new();
    maybe_add_settings_sync_toggle_binding(
        app,
        &id!("Workspace"),
        WorkspaceAction::DispatchToSettingsTab,
        &mut toggle_binding_pairs,
    );
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(toggle_binding_pairs, app);
}

#[derive(Debug, Clone)]
pub enum MainPageAction {
    Relaunch,
    DownloadUpdate,
    CheckForUpdate,
    ToggleSettingsSync,
    OpenUrl(String),
    #[cfg(not(target_family = "wasm"))]
    RefreshIapCredentials,
}

impl MainPageAction {}

#[derive(Clone, Copy)]
pub enum MainSettingsPageEvent {
    CheckForUpdate,
}

pub struct MainSettingsPageView {
    self_handle: WeakViewHandle<Self>,
    page: PageType<Self>,
    auth_state: Arc<AuthState>,
}

impl Entity for MainSettingsPageView {
    type Event = MainSettingsPageEvent;
}

impl TypedActionView for MainSettingsPageView {
    type Action = MainPageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        // LOCAL FORK: a login gate stood here, bouncing anonymous users to a sign-in
        // modal before `Upgrade`, `GenerateStripeBillingPortalLink` or `ToggleSettingsSync`
        // could run. With login gone the gate would have swallowed all three unconditionally.
        // They are billing and settings-sync actions that need a server this build never
        // reaches, so they fail on their own terms instead of behind a prompt.

        match action {
            MainPageAction::Relaunch => {
                autoupdate::initiate_relaunch_for_update(ctx);
            }
            MainPageAction::DownloadUpdate => {
                autoupdate::manually_download_new_version(ctx);
            }
            MainPageAction::CheckForUpdate => {
                ctx.emit(MainSettingsPageEvent::CheckForUpdate);
                ctx.notify();
            }
            MainPageAction::ToggleSettingsSync => {
                let _new_value =
                    CloudPreferencesSettings::handle(ctx).update(ctx, |prefs_settings, ctx| {
                        report_if_error!(
                            prefs_settings
                                .settings_sync_enabled
                                .toggle_and_save_value(ctx)
                        );
                        *prefs_settings.settings_sync_enabled
                    });
                ctx.notify();
            }
            MainPageAction::OpenUrl(url) => {
                ctx.open_url(url);
            }
            #[cfg(not(target_family = "wasm"))]
            MainPageAction::RefreshIapCredentials => {
                IapManager::handle(ctx).update(ctx, |manager, ctx| manager.start_refresh(ctx));
                ctx.notify();
            }
        }
    }
}

impl View for MainSettingsPageView {
    fn ui_name() -> &'static str {
        "MainSettingsPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

impl MainSettingsPageView {
    pub fn new(ctx: &mut ViewContext<MainSettingsPageView>) -> Self {
        let auth_state = AuthStateProvider::as_ref(ctx).get().clone();

        let autoupdate_state_handle = AutoupdateState::handle(ctx);
        ctx.observe(
            &autoupdate_state_handle,
            Self::handle_autoupdate_state_change,
        );

        ctx.subscribe_to_model(&CloudPreferencesSettings::handle(ctx), |_, _, _, ctx| {
            ctx.notify();
        });

        let mut widgets: Vec<Box<dyn SettingsWidget<View = Self>>> =
            vec![Box::new(DividerWidget {})];

        widgets.push(Box::new(SettingsSyncWidget::default()));

        #[cfg(not(target_family = "wasm"))]
        if IapManager::as_ref(ctx).is_enabled() {
            widgets.push(Box::new(IapCredentialsWidget::default()));
            let iap_manager_handle = IapManager::handle(ctx);
            ctx.subscribe_to_model(&iap_manager_handle, |_, _, e, ctx| {
                if matches!(e, IapManagerEvent::StateChanged) {
                    ctx.notify();
                }
            })
        }

        if ChannelState::app_version().is_some() {
            widgets.push(Box::new(VersionInfoWidget::default()));
        }

        let page = PageType::new_uncategorized(widgets, Some("Account"));

        MainSettingsPageView {
            self_handle: ctx.handle(),
            page,
            auth_state,
        }
    }

    fn handle_autoupdate_state_change(
        &mut self,
        _: ModelHandle<AutoupdateState>,
        ctx: &mut ViewContext<Self>,
    ) {
        ctx.notify();
    }
}

// LOCAL FORK: `AccountWidget` went with accounts. It was the settings main page's account
// section: avatar, email, plan badge, a "Compare plans" upgrade link and, for logged-out
// users, a "Sign up" button. Since the build is pinned logged out it rendered its anonymous
// branch unconditionally, so the settings page was still offering an account to create.

struct DividerWidget {}

impl SettingsWidget for DividerWidget {
    type View = MainSettingsPageView;

    fn search_terms(&self) -> &str {
        ""
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        Container::new(
            Container::new(Empty::new().finish())
                .with_border(Border::bottom(1.).with_border_fill(appearance.theme().outline()))
                .finish(),
        )
        .with_margin_top(VERTICAL_MARGIN)
        .finish()
    }
}

#[derive(Default)]
struct SettingsSyncWidget {
    tooltip_state: MouseStateHandle,
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for SettingsSyncWidget {
    type View = MainSettingsPageView;

    fn search_terms(&self) -> &str {
        "settings sync"
    }

    fn should_render(&self, app: &AppContext) -> bool {
        !AuthStateProvider::as_ref(app)
            .get()
            .is_anonymous_or_logged_out()
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let preferences_settings = CloudPreferencesSettings::as_ref(app);

        let label_info = AdditionalInfo {
            mouse_state: self.tooltip_state.clone(),
            on_click_action: Some(MainPageAction::OpenUrl(
                "https://docs.warp.dev/terminal/more-features/settings-sync".into(),
            )),
            secondary_text: None,
            tooltip_override_text: None,
        };

        Container::new(render_body_item::<MainPageAction>(
            "Settings sync".to_string(),
            Some(label_info),
            // Cloud prefs are always synced, so no need to show the local-only icon.
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(*preferences_settings.settings_sync_enabled.value())
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(MainPageAction::ToggleSettingsSync)
                })
                .finish(),
            None,
        ))
        .with_margin_top(VERTICAL_MARGIN)
        .finish()
    }
}

#[derive(Default)]
struct VersionInfoWidget {
    copy_version_button_mouse_state: MouseStateHandle,
    version_info_cta_link_mouse_state: MouseStateHandle,
}

impl VersionInfoWidget {
    fn render_version_info(
        &self,
        version: &'static str,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let faded_text_color = appearance
            .theme()
            .active_ui_text_color()
            .with_opacity(60)
            .into();
        struct StatusContent {
            text: &'static str,
            color: ColorU,
        }
        struct CallToActionContent {
            text: &'static str,
            action: MainPageAction,
        }

        let (status_content, call_to_action_content) =
            if ContextFlag::PromptForVersionUpdates.is_enabled() {
                let ansi_red: ColorU = appearance.theme().terminal_colors().bright.red.into();
                match autoupdate::get_update_state(app) {
                    AutoupdateStage::NoUpdateAvailable => (
                        Some(StatusContent {
                            text: "Up to date",
                            color: faded_text_color,
                        }),
                        Some(CallToActionContent {
                            text: "Check for updates",
                            action: MainPageAction::CheckForUpdate,
                        }),
                    ),
                    AutoupdateStage::CheckingForUpdate => (
                        Some(StatusContent {
                            text: "checking for update...",
                            color: faded_text_color,
                        }),
                        None,
                    ),
                    AutoupdateStage::DownloadingUpdate => (
                        Some(StatusContent {
                            text: "downloading update...",
                            color: faded_text_color,
                        }),
                        None,
                    ),
                    AutoupdateStage::UpdateReady { .. } => (
                        Some(StatusContent {
                            text: "Update available",
                            color: ansi_red,
                        }),
                        Some(CallToActionContent {
                            text: "Relaunch Warp",
                            action: MainPageAction::Relaunch,
                        }),
                    ),
                    AutoupdateStage::Updating { .. } => (
                        Some(StatusContent {
                            text: "Updating...",
                            color: faded_text_color,
                        }),
                        None,
                    ),
                    AutoupdateStage::UpdatedPendingRestart { .. } => (
                        Some(StatusContent {
                            text: "Installed update",
                            color: faded_text_color,
                        }),
                        Some(CallToActionContent {
                            text: "Relaunch Warp",
                            action: MainPageAction::Relaunch,
                        }),
                    ),
                    AutoupdateStage::UnableToUpdateToNewVersion { .. } => (
                        Some(StatusContent {
                            text: "A new version of Warp is available but can't be installed",
                            color: ansi_red,
                        }),
                        Some(CallToActionContent {
                            text: "Update Warp manually",
                            // note: the handler for this action is a no-op
                            action: MainPageAction::DownloadUpdate,
                        }),
                    ),
                    AutoupdateStage::UnableToLaunchNewVersion { .. } => (
                        Some(StatusContent {
                            text: "A new version of Warp is installed but can't be launched.",
                            color: ansi_red,
                        }),
                        Some(CallToActionContent {
                            text: "Update Warp manually",
                            // note: the handler for this action is a no-op
                            action: MainPageAction::DownloadUpdate,
                        }),
                    ),
                }
            } else {
                (None, None)
            };

        let mut first_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(
                Shrinkable::new(
                    1.0,
                    Align::new(
                        Text::new_inline(
                            "Version".to_string(),
                            appearance.ui_font_family(),
                            REGULAR_TEXT_FONT_SIZE,
                        )
                        .with_color(faded_text_color)
                        .finish(),
                    )
                    .left()
                    .finish(),
                )
                .finish(),
            );
        if let Some(call_to_action_content) = call_to_action_content {
            first_row.add_child(
                appearance
                    .ui_builder()
                    .link(
                        call_to_action_content.text.into(),
                        None,
                        Some(Box::new(move |ctx| {
                            ctx.dispatch_typed_action(call_to_action_content.action.clone());
                        })),
                        self.version_info_cta_link_mouse_state.clone(),
                    )
                    .soft_wrap(false)
                    .build()
                    .finish(),
            );
        }

        let mut second_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(
                Shrinkable::new(
                    1.0,
                    Align::new(
                        Flex::row()
                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                            .with_child(
                                appearance
                                    .ui_builder()
                                    .copy_button(16., self.copy_version_button_mouse_state.clone())
                                    .build()
                                    .with_cursor(Cursor::PointingHand)
                                    .on_click(move |ctx, _, _| {
                                        ctx.dispatch_typed_action(WorkspaceAction::CopyVersion(
                                            version,
                                        ));
                                    })
                                    .finish(),
                            )
                            .with_child(
                                Container::new(
                                    Text::new_inline(
                                        version.to_string(),
                                        appearance.ui_font_family(),
                                        REGULAR_TEXT_FONT_SIZE,
                                    )
                                    .with_color(appearance.theme().active_ui_text_color().into())
                                    .finish(),
                                )
                                .with_margin_left(8.)
                                .finish(),
                            )
                            .finish(),
                    )
                    .left()
                    .finish(),
                )
                .finish(),
            );
        if let Some(status_content) = status_content {
            second_row.add_child(
                Text::new_inline(
                    status_content.text.to_string(),
                    appearance.ui_font_family(),
                    REGULAR_TEXT_FONT_SIZE,
                )
                .with_color(status_content.color)
                .finish(),
            );
        }

        let mut version_info = Flex::column();
        version_info.add_child(first_row.finish());
        version_info.add_child(
            Container::new(second_row.finish())
                .with_margin_top(5.)
                .finish(),
        );
        version_info.finish()
    }
}

impl SettingsWidget for VersionInfoWidget {
    type View = MainSettingsPageView;

    fn search_terms(&self) -> &str {
        "version update"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        if let Some(version) = ChannelState::app_version() {
            Container::new(self.render_version_info(version, appearance, app))
                .with_margin_top(VERTICAL_MARGIN)
                .finish()
        } else {
            report_error!("Shouldn't render VersionInfoWidget without GIT_RELEASE_TAG");
            Empty::new().finish()
        }
    }
}

/// Widget displaying IAP credential state and a refresh button. Only
/// visible on staging channels where IAP is active.
#[cfg(not(target_family = "wasm"))]
#[derive(Default)]
struct IapCredentialsWidget {
    refresh_button_mouse_state: MouseStateHandle,
}

#[cfg(not(target_family = "wasm"))]
impl SettingsWidget for IapCredentialsWidget {
    type View = MainSettingsPageView;

    fn search_terms(&self) -> &str {
        "iap staging gcloud proxy credentials"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        // `is_enabled()` gates widget registration in `MainSettingsPageView::new`,
        // so `state()` should be `Some` here; bail out defensively though.
        let Some(state) = IapManager::as_ref(app).state() else {
            return Empty::new().finish();
        };
        let ansi_red: ColorU = appearance.theme().terminal_colors().bright.red.into();
        let disabled: ColorU = appearance.theme().disabled_ui_text_color().into();
        let active: ColorU = appearance.theme().active_ui_text_color().into();
        let (status_text, status_color): (String, ColorU) = match &state {
            IapCredentialsState::Missing => ("Not yet loaded".to_string(), disabled),
            IapCredentialsState::Refreshing { .. } => ("Refreshing…".to_string(), active),
            IapCredentialsState::Loaded(cached) => {
                let remaining = cached
                    .expires_at
                    .saturating_duration_since(instant::Instant::now());
                let mins = remaining.as_secs() / 60;
                (format!("Loaded (refreshes in ~{mins}m)"), active)
            }
            IapCredentialsState::Failed { message, .. } => (format!("Failed: {message}"), ansi_red),
        };

        let is_refreshing = matches!(state, IapCredentialsState::Refreshing { .. });

        let label = Align::new(
            Text::new_inline(
                "Staging IAP credentials".to_string(),
                appearance.ui_font_family(),
                REGULAR_TEXT_FONT_SIZE,
            )
            .with_color(appearance.theme().active_ui_text_color().into())
            .finish(),
        )
        .left()
        .finish();

        let status = Container::new(
            appearance
                .ui_builder()
                .paragraph(status_text)
                .with_style(UiComponentStyles {
                    font_color: Some(status_color),
                    font_size: Some(REGULAR_TEXT_FONT_SIZE),
                    ..Default::default()
                })
                .build()
                .finish(),
        )
        .with_margin_top(4.)
        .finish();

        let refresh_button = appearance
            .ui_builder()
            .button(
                ButtonVariant::Secondary,
                self.refresh_button_mouse_state.clone(),
            )
            .with_text_label(if is_refreshing {
                "Refreshing…".into()
            } else {
                "Refresh".into()
            })
            .with_style(UiComponentStyles {
                font_size: Some(12.),
                padding: Some(Coords::uniform(6.).left(16.).right(16.)),
                ..Default::default()
            })
            .build()
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(MainPageAction::RefreshIapCredentials);
            })
            .finish();

        let button_row = Container::new(Align::new(refresh_button).left().finish())
            .with_margin_top(8.)
            .finish();

        Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_child(label)
                .with_child(status)
                .with_child(button_row)
                .finish(),
        )
        .with_margin_top(VERTICAL_MARGIN)
        .finish()
    }
}

// LOCAL FORK: `LogoutWidget` went with logging out.

impl SettingsPageMeta for MainSettingsPageView {
    fn section() -> SettingsSection {
        SettingsSection::Account
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        true
    }

    fn on_page_selected(&mut self, _: bool, ctx: &mut ViewContext<Self>) {
        // We want to immediately see if the user is part of a workspace rather than wait for the next poll.
        std::mem::drop(
            TeamUpdateManager::handle(ctx)
                .update(ctx, |manager, ctx| manager.refresh_workspace_metadata(ctx)),
        );
    }

    fn update_filter(&mut self, query: &str, ctx: &mut ViewContext<Self>) -> MatchData {
        self.page.update_filter(query, ctx)
    }

    fn scroll_to_widget(&mut self, widget_id: &'static str) {
        self.page.scroll_to_widget(widget_id)
    }

    fn clear_highlighted_widget(&mut self) {
        self.page.clear_highlighted_widget();
    }
}

impl From<ViewHandle<MainSettingsPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<MainSettingsPageView>) -> Self {
        SettingsPageViewHandle::Main(view_handle)
    }
}
