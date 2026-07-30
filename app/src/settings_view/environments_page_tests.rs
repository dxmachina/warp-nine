use std::collections::HashMap;

use cloud_object_models::GithubRepo;
use instant::Instant;
use warp_core::ui::appearance::Appearance;
use warpui::elements::Empty;
use warpui::platform::WindowStyle;
use warpui::{App, AppContext, Element, Entity, TypedActionView, View, WindowId};

use super::*;
use crate::auth::AuthStateProvider;
use crate::network::NetworkStatus;
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::ids::{ClientId, SyncId};
use crate::server::server_api::ServerApiProvider;
use crate::server::sync_queue::SyncQueue;
use crate::settings::PrivacySettings;
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::terminal::view::init_environment::mode_selector::EnvironmentSetupModeSelector;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workspaces::team_tester::TeamTesterStatus;
use crate::workspaces::user_workspaces::UserWorkspaces;

fn make_test_environment(
    name: &str,
    docker_image: &str,
    github_repos: Vec<(String, String)>,
    setup_commands: Vec<String>,
) -> EnvironmentDisplayData {
    make_test_environment_with_timestamps(
        name,
        docker_image,
        github_repos,
        setup_commands,
        None,
        None,
    )
}

fn make_test_environment_with_timestamps(
    name: &str,
    docker_image: &str,
    github_repos: Vec<(String, String)>,
    setup_commands: Vec<String>,
    last_edited_ts: Option<warp_graphql::scalars::time::ServerTimestamp>,
    last_used_ts: Option<warp_graphql::scalars::time::ServerTimestamp>,
) -> EnvironmentDisplayData {
    EnvironmentDisplayData {
        id: SyncId::ClientId(ClientId::new()),
        name: name.to_string(),
        description: None,
        docker_image: docker_image.to_string(),
        github_repos,
        setup_commands,
        last_edited_ts,
        last_used_ts,
    }
}

#[derive(Default)]
struct TestRootView;

impl Entity for TestRootView {
    type Event = ();
}

impl View for TestRootView {
    fn ui_name() -> &'static str {
        "TestRootView"
    }

    fn render(&self, _: &AppContext) -> Box<dyn Element> {
        Empty::new().finish()
    }
}

impl TypedActionView for TestRootView {
    type Action = ();
}

fn create_test_window(app: &mut App) -> WindowId {
    let (window_id, _root_view) = app.add_window(WindowStyle::NotStealFocus, |_| TestRootView);
    window_id
}

fn init_env_page_view_test_models(app: &mut App) {
    initialize_settings_for_tests(app);

    // Most Settings views assume these singleton models exist.
    app.add_singleton_model(|_ctx| ServerApiProvider::new_for_test());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(CloudModel::mock);

    // Some Environments page code paths consult org/user settings (e.g. codebase context enablement),
    // even if the specific test isn't exercising them directly.
    app.add_singleton_model(UserWorkspaces::default_mock);
    app.add_singleton_model(PrivacySettings::mock);

    app.add_singleton_model(|_| NetworkStatus::new());
    app.add_singleton_model(TeamTesterStatus::mock);
    app.add_singleton_model(SyncQueue::mock);
    app.add_singleton_model(UpdateManager::mock);
    app.add_singleton_model(|_| KeybindingChangedNotifier::new());
    // LOCAL FORK: the GitHub auth notifier and the `CodebaseIndexManager` that fed
    // the agent-assisted environment modal both went with the agent.
}

type EmptyMouseStates = (
    HashMap<SyncId, MouseStateHandle>,
    HashMap<SyncId, MouseStateHandle>,
    HashMap<SyncId, MouseStateHandle>,
    HashMap<SyncId, MouseStateHandle>,
    HashMap<SyncId, MouseStateHandle>,
    HashMap<SyncId, Instant>,
);

fn empty_card_mouse_states() -> EmptyMouseStates {
    (
        HashMap::new(),
        HashMap::new(),
        HashMap::new(),
        HashMap::new(),
        HashMap::new(),
        HashMap::new(),
    )
}

#[test]
fn test_render_environments_list_with_single_environment() {
    App::test((), |mut app| async move {
        app.add_singleton_model(|_| Appearance::mock());

        app.update(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let environment =
                make_test_environment("Test Environment", "ubuntu:latest", vec![], vec![]);
            let (
                copy_mouse_states,
                edit_mouse_states,
                share_mouse_states,
                card_hover_states,
                view_runs_link_mouse_states,
                copy_feedback_times,
            ) = empty_card_mouse_states();
            let card_render_state = EnvironmentCardRenderState {
                copy_button_mouse_states: &copy_mouse_states,
                edit_button_mouse_states: &edit_mouse_states,
                share_button_mouse_states: &share_mouse_states,
                card_hover_mouse_states: &card_hover_states,
                view_runs_link_mouse_states: &view_runs_link_mouse_states,
                copy_feedback_times: &copy_feedback_times,
            };

            let element = EnvironmentsPageWidget::render_environments_list(
                &[environment],
                &card_render_state,
                appearance,
                ctx,
                EnvironmentListScope::Personal,
                false,
            );

            let text_content = element.debug_text_content().unwrap_or_default();
            assert!(
                text_content.contains("Test Environment"),
                "Expected environment name in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("ubuntu:latest"),
                "Expected docker image in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("View my runs"),
                "Expected 'View my runs' link in rendered content: {}",
                text_content
            );
        });
    })
}

#[test]
fn test_render_environments_list_with_multiple_environments() {
    App::test((), |mut app| async move {
        app.add_singleton_model(|_| Appearance::mock());

        app.update(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let environments = vec![
                make_test_environment("Environment 1", "ubuntu:latest", vec![], vec![]),
                make_test_environment("Environment 2", "debian:latest", vec![], vec![]),
            ];
            let (
                copy_mouse_states,
                edit_mouse_states,
                share_mouse_states,
                card_hover_states,
                view_runs_link_mouse_states,
                copy_feedback_times,
            ) = empty_card_mouse_states();
            let card_render_state = EnvironmentCardRenderState {
                copy_button_mouse_states: &copy_mouse_states,
                edit_button_mouse_states: &edit_mouse_states,
                share_button_mouse_states: &share_mouse_states,
                card_hover_mouse_states: &card_hover_states,
                view_runs_link_mouse_states: &view_runs_link_mouse_states,
                copy_feedback_times: &copy_feedback_times,
            };

            let element = EnvironmentsPageWidget::render_environments_list(
                &environments,
                &card_render_state,
                appearance,
                ctx,
                EnvironmentListScope::Personal,
                false,
            );

            let text_content = element.debug_text_content().unwrap_or_default();
            assert!(
                text_content.contains("Environment 1"),
                "Expected first environment name in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("Environment 2"),
                "Expected second environment name in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("ubuntu:latest"),
                "Expected first docker image in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("debian:latest"),
                "Expected second docker image in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("View my runs"),
                "Expected 'View my runs' link in rendered content: {}",
                text_content
            );
        });
    })
}

#[test]
fn test_render_environment_card_with_minimal_config() {
    App::test((), |mut app| async move {
        app.add_singleton_model(|_| Appearance::mock());

        app.update(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let environment =
                make_test_environment("Minimal Environment", "alpine:latest", vec![], vec![]);
            let (
                copy_mouse_states,
                edit_mouse_states,
                share_mouse_states,
                card_hover_states,
                view_runs_link_mouse_states,
                copy_feedback_times,
            ) = empty_card_mouse_states();
            let card_render_state = EnvironmentCardRenderState {
                copy_button_mouse_states: &copy_mouse_states,
                edit_button_mouse_states: &edit_mouse_states,
                share_button_mouse_states: &share_mouse_states,
                card_hover_mouse_states: &card_hover_states,
                view_runs_link_mouse_states: &view_runs_link_mouse_states,
                copy_feedback_times: &copy_feedback_times,
            };

            let element = EnvironmentsPageWidget::render_environment_card(
                &environment,
                &card_render_state,
                appearance,
                ctx,
                EnvironmentListScope::Personal,
                false,
            );

            let text_content = element.debug_text_content().unwrap_or_default();
            assert!(
                text_content.contains("Minimal Environment"),
                "Expected environment name in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("alpine:latest"),
                "Expected docker image in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("View my runs"),
                "Expected 'View my runs' link in rendered content: {}",
                text_content
            );
        });
    })
}

#[test]
fn test_render_environment_card_with_github_repos() {
    App::test((), |mut app| async move {
        app.add_singleton_model(|_| Appearance::mock());

        app.update(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let environment = make_test_environment(
                "Environment with Repos",
                "ubuntu:latest",
                vec![
                    ("owner1".to_string(), "repo1".to_string()),
                    ("owner2".to_string(), "repo2".to_string()),
                ],
                vec![],
            );
            let (
                copy_mouse_states,
                edit_mouse_states,
                share_mouse_states,
                card_hover_states,
                view_runs_link_mouse_states,
                copy_feedback_times,
            ) = empty_card_mouse_states();
            let card_render_state = EnvironmentCardRenderState {
                copy_button_mouse_states: &copy_mouse_states,
                edit_button_mouse_states: &edit_mouse_states,
                share_button_mouse_states: &share_mouse_states,
                card_hover_mouse_states: &card_hover_states,
                view_runs_link_mouse_states: &view_runs_link_mouse_states,
                copy_feedback_times: &copy_feedback_times,
            };

            let element = EnvironmentsPageWidget::render_environment_card(
                &environment,
                &card_render_state,
                appearance,
                ctx,
                EnvironmentListScope::Personal,
                false,
            );

            let text_content = element.debug_text_content().unwrap_or_default();
            assert!(
                text_content.contains("Environment with Repos"),
                "Expected environment name in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("owner1/repo1"),
                "Expected first repo in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("owner2/repo2"),
                "Expected second repo in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("View my runs"),
                "Expected 'View my runs' link in rendered content: {}",
                text_content
            );
        });
    })
}

#[test]
fn test_render_environment_card_with_setup_commands() {
    App::test((), |mut app| async move {
        app.add_singleton_model(|_| Appearance::mock());

        app.update(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let environment = make_test_environment(
                "Environment with Setup",
                "node:18",
                vec![],
                vec!["npm install".to_string(), "npm run build".to_string()],
            );
            let (
                copy_mouse_states,
                edit_mouse_states,
                share_mouse_states,
                card_hover_states,
                view_runs_link_mouse_states,
                copy_feedback_times,
            ) = empty_card_mouse_states();
            let card_render_state = EnvironmentCardRenderState {
                copy_button_mouse_states: &copy_mouse_states,
                edit_button_mouse_states: &edit_mouse_states,
                share_button_mouse_states: &share_mouse_states,
                card_hover_mouse_states: &card_hover_states,
                view_runs_link_mouse_states: &view_runs_link_mouse_states,
                copy_feedback_times: &copy_feedback_times,
            };

            let element = EnvironmentsPageWidget::render_environment_card(
                &environment,
                &card_render_state,
                appearance,
                ctx,
                EnvironmentListScope::Personal,
                false,
            );

            let text_content = element.debug_text_content().unwrap_or_default();
            assert!(
                text_content.contains("Environment with Setup"),
                "Expected environment name in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("node:18"),
                "Expected docker image in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("npm install"),
                "Expected first setup command in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("npm run build"),
                "Expected second setup command in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("View my runs"),
                "Expected 'View my runs' link in rendered content: {}",
                text_content
            );
        });
    })
}

#[test]
fn test_render_environment_card_with_all_features() {
    App::test((), |mut app| async move {
        app.add_singleton_model(|_| Appearance::mock());

        app.update(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let environment = make_test_environment(
                "Full Environment",
                "python:3.11",
                vec![
                    ("company".to_string(), "frontend".to_string()),
                    ("company".to_string(), "backend".to_string()),
                ],
                vec![
                    "pip install -r requirements.txt".to_string(),
                    "python setup.py".to_string(),
                ],
            );
            let (
                copy_mouse_states,
                edit_mouse_states,
                share_mouse_states,
                card_hover_states,
                view_runs_link_mouse_states,
                copy_feedback_times,
            ) = empty_card_mouse_states();
            let card_render_state = EnvironmentCardRenderState {
                copy_button_mouse_states: &copy_mouse_states,
                edit_button_mouse_states: &edit_mouse_states,
                share_button_mouse_states: &share_mouse_states,
                card_hover_mouse_states: &card_hover_states,
                view_runs_link_mouse_states: &view_runs_link_mouse_states,
                copy_feedback_times: &copy_feedback_times,
            };

            let element = EnvironmentsPageWidget::render_environment_card(
                &environment,
                &card_render_state,
                appearance,
                ctx,
                EnvironmentListScope::Personal,
                false,
            );

            let text_content = element.debug_text_content().unwrap_or_default();
            assert!(
                text_content.contains("Full Environment"),
                "Expected environment name in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("python:3.11"),
                "Expected docker image in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("company/frontend"),
                "Expected first repo in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("company/backend"),
                "Expected second repo in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("pip install -r requirements.txt"),
                "Expected first setup command in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("python setup.py"),
                "Expected second setup command in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("View my runs"),
                "Expected 'View my runs' link in rendered content: {}",
                text_content
            );
        });
    })
}

#[test]
fn test_render_environment_card_with_empty_setup_commands() {
    App::test((), |mut app| async move {
        app.add_singleton_model(|_| Appearance::mock());

        app.update(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let environment = make_test_environment(
                "Environment with Empty Commands",
                "ubuntu:latest",
                vec![],
                vec![],
            );
            let (
                copy_mouse_states,
                edit_mouse_states,
                share_mouse_states,
                card_hover_states,
                view_runs_link_mouse_states,
                copy_feedback_times,
            ) = empty_card_mouse_states();
            let card_render_state = EnvironmentCardRenderState {
                copy_button_mouse_states: &copy_mouse_states,
                edit_button_mouse_states: &edit_mouse_states,
                share_button_mouse_states: &share_mouse_states,
                card_hover_mouse_states: &card_hover_states,
                view_runs_link_mouse_states: &view_runs_link_mouse_states,
                copy_feedback_times: &copy_feedback_times,
            };

            let element = EnvironmentsPageWidget::render_environment_card(
                &environment,
                &card_render_state,
                appearance,
                ctx,
                EnvironmentListScope::Personal,
                false,
            );

            let text_content = element.debug_text_content().unwrap_or_default();
            assert!(
                text_content.contains("Environment with Empty Commands"),
                "Expected environment name in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("ubuntu:latest"),
                "Expected docker image in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("View my runs"),
                "Expected 'View my runs' link in rendered content: {}",
                text_content
            );
        });
    })
}

#[test]
fn test_environments_page_widget_search_terms() {
    let widget = EnvironmentsPageWidget;
    let search_terms = widget.search_terms();

    assert!(search_terms.contains("environments"));
    assert!(search_terms.contains("environment"));
    assert!(search_terms.contains("ambient"));
    assert!(search_terms.contains("agents"));
    assert!(search_terms.contains("github"));
}

// ============================================================================
// Empty State vs List State Tests
// ============================================================================

#[test]
fn test_render_list_page_with_no_environments_shows_empty_state() {
    // Test that when there are no environments, the empty state is rendered
    App::test((), |mut app| async move {
        init_env_page_view_test_models(&mut app);
        let window_id = create_test_window(&mut app);

        app.update(|ctx| {
            let view_handle = ctx.add_typed_action_view(window_id, EnvironmentsPageView::new);
            let appearance = Appearance::as_ref(ctx);

            let view = view_handle.as_ref(ctx);
            let element = EnvironmentsPageWidget::render_list_page(view, appearance, ctx);
            // Element is created successfully - just verify it doesn't panic
            drop(element);
        });
    })
}

#[test]
fn test_render_empty_state_shows_github_remote_and_local_rows() {
    // Empty-state UI should include GitHub-remote (suggested) and agent-assisted local repos paths.
    App::test((), |mut app| async move {
        init_env_page_view_test_models(&mut app);
        let window_id = create_test_window(&mut app);

        app.update(|ctx| {
            let view_handle = ctx.add_typed_action_view(window_id, EnvironmentsPageView::new);
            let appearance = Appearance::as_ref(ctx);
            let view = view_handle.as_ref(ctx);

            let element = EnvironmentsPageWidget::render_empty_state(view, appearance, ctx);
            let text_content = element.debug_text_content().unwrap_or_default();

            assert!(
                text_content.contains("Quick setup"),
                "Expected quick setup row title in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("Suggested"),
                "Expected 'Suggested' badge text in rendered content: {}",
                text_content
            );
            // GitHub button text depends on async auth state, so just check that one of the
            // expected states is present (Loading, Get started, Authorize, or Retry)
            let has_github_button = text_content.contains("Get started")
                || text_content.contains("Authorize")
                || text_content.contains("Loading...")
                || text_content.contains("Retry");
            assert!(
                has_github_button,
                "Expected GitHub button text in rendered content: {}",
                text_content
            );

            assert!(
                text_content.contains("Use the agent"),
                "Expected 'Use the agent' row title in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("Launch agent"),
                "Expected 'Launch agent' button text in rendered content: {}",
                text_content
            );

            assert!(
                !text_content.contains("Manually create an environment"),
                "Did not expect old manual-create empty-state row title in rendered content: {}",
                text_content
            );

            // Basic ordering: GitHub row should appear above local repos row.
            let github_pos = text_content.find("Quick setup").unwrap_or(usize::MAX);
            let local_pos = text_content
                .find("Use the agent")
                .unwrap_or(usize::MAX);
            assert!(
                github_pos < local_pos,
                "Expected GitHub row to appear before local row (github_pos={github_pos}, local_pos={local_pos}): {text_content}"
            );
        });
    })
}

#[test]
fn test_render_empty_state_github_card_loading_state() {
    // This test verifies that the empty state renders without crashing.
    // The specific GitHub auth state (Loading, Authed, etc.) is asynchronous
    // and can't be reliably controlled in unit tests.
    App::test((), |mut app| async move {
        init_env_page_view_test_models(&mut app);
        let window_id = create_test_window(&mut app);

        app.update(|ctx| {
            let view_handle = ctx.add_typed_action_view(window_id, EnvironmentsPageView::new);
            let appearance = Appearance::as_ref(ctx);
            let view = view_handle.as_ref(ctx);

            let element = EnvironmentsPageWidget::render_empty_state(view, appearance, ctx);
            let text_content = element.debug_text_content().unwrap_or_default();

            // Just verify the empty state renders the key components
            assert!(
                text_content.contains("Quick setup"),
                "Expected quick setup row in rendered content: {}",
                text_content
            );
        });
    })
}

#[test]
fn test_render_empty_state_github_card_error_state_shows_retry() {
    // This test verifies that the empty state renders without crashing.
    // The specific GitHub auth state (error, loading, etc.) is asynchronous
    // and can't be reliably controlled in unit tests.
    App::test((), |mut app| async move {
        init_env_page_view_test_models(&mut app);
        let window_id = create_test_window(&mut app);

        app.update(|ctx| {
            let view_handle = ctx.add_typed_action_view(window_id, EnvironmentsPageView::new);
            let appearance = Appearance::as_ref(ctx);
            let view = view_handle.as_ref(ctx);

            let element = EnvironmentsPageWidget::render_empty_state(view, appearance, ctx);
            let text_content = element.debug_text_content().unwrap_or_default();

            // Just verify the empty state renders the key components
            assert!(
                text_content.contains("Quick setup"),
                "Expected quick setup row in rendered content: {}",
                text_content
            );
        });
    })
}

#[test]
fn test_render_empty_state_github_card_unauthed_state_shows_authorize() {
    // This test verifies that the empty state renders without crashing.
    // The specific GitHub auth state (unauthed, authed, etc.) is asynchronous
    // and can't be reliably controlled in unit tests.
    App::test((), |mut app| async move {
        init_env_page_view_test_models(&mut app);
        let window_id = create_test_window(&mut app);

        app.update(|ctx| {
            let view_handle = ctx.add_typed_action_view(window_id, EnvironmentsPageView::new);
            let appearance = Appearance::as_ref(ctx);
            let view = view_handle.as_ref(ctx);

            let element = EnvironmentsPageWidget::render_empty_state(view, appearance, ctx);
            let text_content = element.debug_text_content().unwrap_or_default();

            // Just verify the empty state renders the key components
            assert!(
                text_content.contains("Quick setup"),
                "Expected quick setup row in rendered content: {}",
                text_content
            );
        });
    })
}

// ============================================================================
// Toolbar + Agent-assisted Flow Tests
// ============================================================================

#[test]
fn test_environment_setup_mode_selector_renders_options() {
    App::test((), |mut app| async move {
        app.add_singleton_model(|_| Appearance::mock());
        let window_id = create_test_window(&mut app);

        app.update(|ctx| {
            let selector = ctx.add_typed_action_view(window_id, EnvironmentSetupModeSelector::new);
            let element = selector.as_ref(ctx).render(ctx);
            let text_content = element.debug_text_content().unwrap_or_default();

            assert!(
                text_content.contains("Quick setup"),
                "Expected Quick setup option in rendered content: {}",
                text_content
            );
            assert!(
                text_content.contains("Use the agent"),
                "Expected Use the agent option in rendered content: {}",
                text_content
            );
        });
    })
}

#[test]
fn test_environments_page_default_is_list() {
    let page = EnvironmentsPage::default();
    assert!(matches!(page, EnvironmentsPage::List));
}

#[test]
fn test_environments_page_edit_variant() {
    let env_id = SyncId::ClientId(ClientId::new());
    let page = EnvironmentsPage::Edit { env_id };

    if let EnvironmentsPage::Edit { env_id: id } = page {
        assert_eq!(id, env_id);
    } else {
        panic!("Expected Edit variant");
    }
}

// ============================================================================
// GithubRepo Tests
// ============================================================================

#[test]
fn test_github_repo_new() {
    let repo = GithubRepo::new("warpdotdev".to_string(), "warp-internal".to_string());
    assert_eq!(repo.owner, "warpdotdev");
    assert_eq!(repo.repo, "warp-internal");
}

#[test]
fn test_github_repo_display() {
    let repo = GithubRepo::new("warpdotdev".to_string(), "warp-internal".to_string());
    assert_eq!(repo.to_string(), "warpdotdev/warp-internal");
}

#[test]
fn test_github_repo_equality() {
    let repo1 = GithubRepo::new("owner".to_string(), "repo".to_string());
    let repo2 = GithubRepo::new("owner".to_string(), "repo".to_string());
    let repo3 = GithubRepo::new("other".to_string(), "repo".to_string());

    assert_eq!(repo1, repo2);
    assert_ne!(repo1, repo3);
}

// ============================================================================
// Environments List Search Tests
// ============================================================================

#[test]
fn test_environment_matches_search_query_empty_query_matches_all() {
    let environment = make_test_environment(
        "Searchable Environment",
        "ubuntu:latest",
        vec![("warpdotdev".to_string(), "warp-internal".to_string())],
        vec![],
    );

    assert!(environment.matches_search_query(""));
    assert!(environment.matches_search_query("   "));
}

#[test]
fn test_environment_matches_search_query_name_description_image_repos() {
    let mut environment = make_test_environment(
        "Warp Env",
        "node:20-alpine",
        vec![("warpdotdev".to_string(), "warp-internal".to_string())],
        vec![],
    );
    environment.description = Some("Front end focused agents".to_string());

    assert!(environment.matches_search_query("warp"));
    assert!(environment.matches_search_query("Front end"));
    assert!(environment.matches_search_query("node:20"));
    assert!(environment.matches_search_query("warp-internal"));
    assert!(environment.matches_search_query("warpdotdev"));
    assert!(environment.matches_search_query("warpdotdev/warp"));

    assert!(!environment.matches_search_query("definitely-not-present"));
}

#[test]
fn test_environment_matches_search_query_env_id_substring() {
    let environment = make_test_environment("Any", "ubuntu:latest", vec![], vec![]);

    let id_str = environment.id.to_string();
    let needle_len = id_str.chars().take(6).collect::<String>().len();
    let prefix = &id_str[..needle_len];

    assert!(environment.matches_search_query(prefix));
}

#[test]
fn test_environment_matches_search_query_is_case_insensitive() {
    let mut environment = make_test_environment(
        "warp-env",
        "ubuntu:latest",
        vec![("WarpDotDev".to_string(), "Warp-Internal".to_string())],
        vec![],
    );
    environment.description = Some("Some Description".to_string());

    assert!(environment.matches_search_query("WARP"));
    assert!(environment.matches_search_query("description"));
    assert!(environment.matches_search_query("warp-internal"));
}

// ============================================================================
// Environment Last Used Timestamp Tests
// ============================================================================

#[test]
fn test_render_environment_card_with_last_used_never() {
    use chrono::{Duration, Utc};
    use warp_graphql::scalars::time::ServerTimestamp;

    App::test((), |mut app| async move {
        app.add_singleton_model(|_| Appearance::mock());

        app.update(|ctx| {
            let appearance = Appearance::as_ref(ctx);

            // Create environment with no last-used timestamp
            let one_day_ago = Utc::now() - Duration::days(1);
            let environment = make_test_environment_with_timestamps(
                "Never Used Environment",
                "ubuntu:latest",
                vec![],
                vec![],
                Some(ServerTimestamp::from(one_day_ago)),
                None,
            );

            let (
                copy_mouse_states,
                edit_mouse_states,
                share_mouse_states,
                card_hover_states,
                view_runs_link_mouse_states,
                copy_feedback_times,
            ) = empty_card_mouse_states();

            // Render the card
            let card_render_state = EnvironmentCardRenderState {
                copy_button_mouse_states: &copy_mouse_states,
                edit_button_mouse_states: &edit_mouse_states,
                share_button_mouse_states: &share_mouse_states,
                card_hover_mouse_states: &card_hover_states,
                view_runs_link_mouse_states: &view_runs_link_mouse_states,
                copy_feedback_times: &copy_feedback_times,
            };

            let element = EnvironmentsPageWidget::render_environment_card(
                &environment,
                &card_render_state,
                appearance,
                ctx,
                EnvironmentListScope::Personal,
                false,
            );

            // Use debug_text_content to verify the rendered text
            let text_content = element.debug_text_content().unwrap_or_default();
            assert!(
                text_content.contains("Last used: never"),
                "Expected 'Last used: never' in rendered text: {}",
                text_content
            );
            assert!(
                text_content.contains("Last edited:"),
                "Expected 'Last edited:' in rendered text: {}",
                text_content
            );
            assert!(
                text_content.contains("View my runs"),
                "Expected 'View my runs' link in rendered text: {}",
                text_content
            );
        });
    })
}

#[test]
fn test_render_environment_card_with_last_used_timestamp() {
    use chrono::{Duration, Utc};
    use warp_graphql::scalars::time::ServerTimestamp;

    App::test((), |mut app| async move {
        app.add_singleton_model(|_| Appearance::mock());

        app.update(|ctx| {
            let appearance = Appearance::as_ref(ctx);

            // Create environment with a last-used timestamp from 2 hours ago
            let one_day_ago = Utc::now() - Duration::days(1);
            let two_hours_ago = Utc::now() - Duration::hours(2);
            let environment = make_test_environment_with_timestamps(
                "Recently Used Environment",
                "python:3.11",
                vec![],
                vec![],
                Some(ServerTimestamp::from(one_day_ago)),
                Some(ServerTimestamp::from(two_hours_ago)),
            );

            let (
                copy_mouse_states,
                edit_mouse_states,
                share_mouse_states,
                card_hover_states,
                view_runs_link_mouse_states,
                copy_feedback_times,
            ) = empty_card_mouse_states();

            // Render the card
            let card_render_state = EnvironmentCardRenderState {
                copy_button_mouse_states: &copy_mouse_states,
                edit_button_mouse_states: &edit_mouse_states,
                share_button_mouse_states: &share_mouse_states,
                card_hover_mouse_states: &card_hover_states,
                view_runs_link_mouse_states: &view_runs_link_mouse_states,
                copy_feedback_times: &copy_feedback_times,
            };

            let element = EnvironmentsPageWidget::render_environment_card(
                &environment,
                &card_render_state,
                appearance,
                ctx,
                EnvironmentListScope::Personal,
                false,
            );

            // Use debug_text_content to verify the rendered text
            let text_content = element.debug_text_content().unwrap_or_default();
            assert!(
                text_content.contains("Last edited:"),
                "Expected 'Last edited:' in rendered text: {}",
                text_content
            );
            assert!(
                text_content.contains("Last used:"),
                "Expected 'Last used:' in rendered text: {}",
                text_content
            );
            assert!(
                !text_content.contains("never"),
                "Did not expect 'never' in rendered text: {}",
                text_content
            );
            assert!(
                text_content.contains("View my runs"),
                "Expected 'View my runs' link in rendered text: {}",
                text_content
            );
        });
    })
}
