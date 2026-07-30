use crate::settings::ai::DefaultSessionMode;
use crate::settings::{AISettings, CodeSettings};
use onboarding::slides::{AgentAutonomy, AgentDevelopmentSettings};
use onboarding::{SelectedSettings, SessionDefault, UICustomizationSettings};
use settings::Setting as _;
use warp_core::features::FeatureFlag;
use warp_errors::report_if_error;
use warpui::{AppContext, SingletonEntity as _};

use crate::drive::settings::WarpDriveSettings;
use crate::workspace::tab_settings::TabSettings;
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::workspaces::workspace::FtueAccountClass;

pub fn apply_account_first_onboarding_settings(
    selected_settings: &SelectedSettings,
    account_class: Option<FtueAccountClass>,
    app: &mut AppContext,
) {
    // Every authenticated account-first user gets the Warp Agent surface,
    // including standard-free accounts with no included Warp credits. Skipping
    // account creation is the only outcome that leaves Agent disabled.
    let is_ai_enabled = match account_class {
        None => false,
        Some(
            FtueAccountClass::Paid | FtueAccountClass::FreeIcp | FtueAccountClass::FreeStandard,
        ) => true,
    };

    match selected_settings {
        SelectedSettings::AgentDrivenDevelopment {
            ui_customization, ..
        } => {
            // LOCAL FORK: apply_agent_settings went away with the agent.
            if let Some(ui) = ui_customization {
                apply_ui_customization_settings(ui, true, app);
            }
        }
        SelectedSettings::Terminal {
            ui_customization,
            cli_agent_toolbar_enabled,
            show_agent_notifications,
        } => {
            if let Some(ui) = ui_customization {
                apply_ui_customization_settings(ui, false, app);
            }
            AISettings::handle(app).update(app, |settings, ctx| {
                report_if_error!(
                    settings
                        .should_render_cli_agent_footer
                        .set_value(*cli_agent_toolbar_enabled, ctx)
                );
                report_if_error!(
                    settings
                        .show_agent_notifications
                        .set_value(*show_agent_notifications, ctx)
                );
            });
        }
    }

    AISettings::handle(app).update(app, |settings, ctx| {
        report_if_error!(settings.is_any_ai_enabled.set_value(is_ai_enabled, ctx));
    });
}

/// Applies onboarding settings based on the user's selected mode.
///
/// `has_account` indicates whether the user has (or is creating) a real Warp
/// account. Warp's AI features run on a Warp account, so agent intent only
/// enables AI when `has_account` is true; skipping login leaves AI off.
pub fn apply_onboarding_settings(
    selected_settings: &SelectedSettings,
    has_account: bool,
    app: &mut AppContext,
) {
    let is_ai_enabled = match selected_settings {
        SelectedSettings::AgentDrivenDevelopment {
            ui_customization, ..
        } => {
            // LOCAL FORK: apply_agent_settings went away with the agent.
            if let Some(ui) = ui_customization {
                apply_ui_customization_settings(ui, true, app);
            }
            // Agent intent means the user wants AI, but Warp's AI features run
            // on a Warp account, so AI is only enabled once they have one.
            // Skipping login leaves AI off even for agent intent (including the
            // bring-your-own-agents `disable_oz` path).
            has_account
        }
        SelectedSettings::Terminal {
            ui_customization,
            cli_agent_toolbar_enabled,
            show_agent_notifications,
        } => {
            // In old onboarding, there's nothing to set for terminal intent.
            if !FeatureFlag::OpenWarpNewSettingsModes.is_enabled() {
                true
            } else {
                if let Some(ui) = ui_customization {
                    apply_ui_customization_settings(ui, false, app);
                }
                AISettings::handle(app).update(app, |settings, ctx| {
                    report_if_error!(
                        settings
                            .should_render_cli_agent_footer
                            .set_value(*cli_agent_toolbar_enabled, ctx)
                    );
                    report_if_error!(
                        settings
                            .show_agent_notifications
                            .set_value(*show_agent_notifications, ctx)
                    );
                });
                false
            }
        }
    };

    if FeatureFlag::OpenWarpNewSettingsModes.is_enabled() {
        AISettings::handle(app).update(app, |settings, ctx| {
            report_if_error!(settings.is_any_ai_enabled.set_value(is_ai_enabled, ctx));
        });
    }
}

/// Applies the explicit UI customization settings chosen during the
/// "Customize your UI" onboarding slide.
fn apply_ui_customization_settings(
    ui: &UICustomizationSettings,
    is_agent_intent: bool,
    app: &mut AppContext,
) {
    // Customize UI slide should only exist with this flag enabled.
    if !FeatureFlag::AccountFirstOnboarding.is_enabled()
        && !FeatureFlag::OpenWarpNewSettingsModes.is_enabled()
    {
        return;
    }
    TabSettings::handle(app).update(app, |settings, ctx| {
        report_if_error!(
            settings
                .use_vertical_tabs
                .set_value(ui.use_vertical_tabs, ctx)
        );
        report_if_error!(
            settings
                .show_code_review_button
                .set_value(ui.show_code_review_button, ctx)
        );
    });

    WarpDriveSettings::handle(app).update(app, |settings, ctx| {
        report_if_error!(
            settings
                .enable_warp_drive
                .set_value(ui.show_warp_drive, ctx)
        );
    });

    CodeSettings::handle(app).update(app, |settings, ctx| {
        report_if_error!(
            settings
                .show_project_explorer
                .set_value(ui.show_project_explorer, ctx)
        );
        report_if_error!(
            settings
                .show_global_search
                .set_value(ui.show_global_search, ctx)
        );
    });

    // For agent intent, configure showing conversation history.
    // For terminal intent, this option was not surfaced in onboarding, so leave the default.
    // It will be hidden anyway because AI is off, but we want to keep the default in case they enable AI later.
    if is_agent_intent {
        AISettings::handle(app).update(app, |settings, ctx| {
            report_if_error!(
                settings
                    .show_conversation_history
                    .set_value(ui.show_conversation_history, ctx)
            );
        });
    }
}


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OnboardingAutonomyPermissions {
}


#[cfg(test)]
#[path = "onboarding_tests.rs"]
mod tests;
