use onboarding::slides::{AgentDevelopmentSettings, ProjectOnboardingSettings};
use onboarding::{SelectedSettings, UICustomizationSettings};
use warp_core::features::FeatureFlag;
use warpui::{App, SingletonEntity};

use crate::auth::AuthStateProvider;
use crate::cloud_object::model::persistence::CloudModel;
use crate::network::NetworkStatus;
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::sync_queue::SyncQueue;
use crate::settings::{
    AISettings, CodeSettings, PrivacySettings, apply_account_first_onboarding_settings,
    apply_onboarding_settings,
};
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workspace::tab_settings::TabSettings;
use crate::workspaces::team_tester::TeamTesterStatus;
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::workspaces::workspace::FtueAccountClass;

// LOCAL FORK: `apply_onboarding_settings_preserves_existing_cloud_profile_on_existing_user_login`
// went out with `crate::ai::execution_profiles`. It covered onboarding not
// clobbering a returning user's cloud-stored AI execution profile — a regression
// that can no longer happen, because there are no execution profiles.

#[test]
fn account_first_settings_enable_agent_for_authenticated_users_and_apply_ui_choices() {
    let _account_first = FeatureFlag::AccountFirstOnboarding.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        app.add_singleton_model(|_| AuthStateProvider::new_for_test());
        app.add_singleton_model(SyncQueue::mock);
        app.add_singleton_model(|_| NetworkStatus::new());
        app.add_singleton_model(TeamTesterStatus::mock);
        app.add_singleton_model(UpdateManager::mock);
        app.add_singleton_model(CloudModel::mock);
        app.add_singleton_model(PrivacySettings::mock);
        app.add_singleton_model(UserWorkspaces::default_mock);

        let selected_settings = SelectedSettings::AgentDrivenDevelopment {
            agent_settings: AgentDevelopmentSettings {
                // LOCAL FORK: built with `into()` so the test does not have to name
                // `ai::LLMId`; `onboarding` still owns the field's type.
                selected_model_id: "auto".into(),
                autonomy: None,
                cli_agent_toolbar_enabled: true,
                session_default: onboarding::SessionDefault::Agent,
                disable_oz: false,
                show_agent_notifications: true,
            },
            project_settings: ProjectOnboardingSettings::default(),
            ui_customization: Some(UICustomizationSettings {
                use_vertical_tabs: false,
                show_conversation_history: false,
                show_project_explorer: true,
                show_global_search: false,
                show_code_review_button: true,
            }),
        };

        for (account_class, expected_ai) in [
            (None, false),
            (Some(FtueAccountClass::FreeStandard), true),
            (Some(FtueAccountClass::FreeIcp), true),
            (Some(FtueAccountClass::Paid), true),
        ] {
            app.update(|ctx| {
                apply_account_first_onboarding_settings(&selected_settings, account_class, ctx);
            });
            app.read(|ctx| {
                assert_eq!(*AISettings::as_ref(ctx).is_any_ai_enabled, expected_ai);
                assert!(!*TabSettings::as_ref(ctx).use_vertical_tabs);
                assert!(*TabSettings::as_ref(ctx).show_code_review_button);
                assert!(*CodeSettings::as_ref(ctx).show_project_explorer);
                assert!(!*CodeSettings::as_ref(ctx).show_global_search);
            });
        }
    });
}

/// Warp's AI features run on a Warp account. For third-party agent intent
/// (`disable_oz = true`), AI is therefore off when the user skips creating an
/// account and on once they have one.
#[test]
fn apply_onboarding_settings_gates_third_party_ai_on_account() {
    let _flag = FeatureFlag::OpenWarpNewSettingsModes.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        app.add_singleton_model(|_| AuthStateProvider::new_for_test());
        app.add_singleton_model(SyncQueue::mock);
        app.add_singleton_model(|_| NetworkStatus::new());
        app.add_singleton_model(TeamTesterStatus::mock);
        app.add_singleton_model(UpdateManager::mock);
        app.add_singleton_model(CloudModel::mock);
        app.add_singleton_model(PrivacySettings::mock);
        app.add_singleton_model(UserWorkspaces::default_mock);

        let onboarding_settings = SelectedSettings::AgentDrivenDevelopment {
            agent_settings: AgentDevelopmentSettings {
                // LOCAL FORK: built with `into()` so the test does not have to name
                // `ai::LLMId`; `onboarding` still owns the field's type.
                selected_model_id: "auto".into(),
                autonomy: None,
                cli_agent_toolbar_enabled: true,
                session_default: onboarding::SessionDefault::Agent,
                disable_oz: true,
                show_agent_notifications: true,
            },
            project_settings: ProjectOnboardingSettings::default(),
            ui_customization: None,
        };

        // Skipping login (no account) leaves AI off, even for agent intent.
        app.update(|ctx| {
            apply_onboarding_settings(&onboarding_settings, false, ctx);
        });
        let ai_disabled = app.read(|ctx| !*AISettings::as_ref(ctx).is_any_ai_enabled);
        assert!(
            ai_disabled,
            "skipping login must disable AI even for agent intent"
        );

        // Creating an account turns AI on, including for third-party agents.
        app.update(|ctx| {
            apply_onboarding_settings(&onboarding_settings, true, ctx);
        });
        let ai_enabled = app.read(|ctx| *AISettings::as_ref(ctx).is_any_ai_enabled);
        assert!(
            ai_enabled,
            "creating an account must enable AI for third-party agent intent"
        );
    })
}
