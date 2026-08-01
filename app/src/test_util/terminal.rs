#[cfg(feature = "local_fs")]
use repo_metadata::RepoMetadataModel;
use repo_metadata::repositories::DetectedRepositories;
use repo_metadata::watcher::DirectoryWatcher;
use warp_core::ui::appearance::Appearance;
use warp_server_client::iap::IapManager;
use warpui::platform::WindowStyle;
use warpui::{App, SingletonEntity, ViewHandle, WindowId};
use watcher::HomeDirectoryWatcher;

use super::settings::initialize_history_persistence_for_tests;
use crate::auth::AuthStateProvider;
use crate::changelog_model::ChangelogModel;
use crate::cloud_object::model::persistence::CloudModel;
use crate::code_review::git_repo_model::GitRepoModels;
use crate::context_chips::prompt::Prompt;
use crate::experiments;
use crate::network::NetworkStatus;
use crate::persisted_workspace::PersistedWorkspace;
use crate::pricing::PricingInfoModel;
use crate::search::files::model::FileSearchModel;
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::server_api::ServerApiProvider;
use crate::server::sync_queue::SyncQueue;
use crate::settings::PrivacySettings;
use crate::settings_view::keybindings::KeybindingChangedNotifier;
#[cfg(feature = "local_fs")]
use crate::skills::SKILL_PROVIDER_DEFINITIONS;
use crate::suggestions::ignored_suggestions_model::IgnoredSuggestionsModel;
use crate::system::{SystemInfo, SystemStats};
use crate::terminal::alt_screen_reporting::AltScreenReporting;
use crate::terminal::keys::TerminalKeybindings;
use crate::terminal::model::block::SerializedBlock;
use crate::terminal::resizable_data::ResizableData;
use crate::terminal::shared_session::permissions_manager::SessionPermissionsManager;
use crate::terminal::view::inline_banner::ByoLlmAuthBannerSessionState;
use crate::terminal::{History, TerminalView};
use crate::undo_close::UndoCloseStack;
use crate::warp_managed_paths_watcher::WarpManagedPathsWatcher;
use crate::workflows::local_workflows::LocalWorkflows;
use crate::workspace::sync_inputs::SyncedInputState;
use crate::workspace::{ActiveSession, OneTimeModalModel, WorkspaceRegistry};
use crate::workspaces::team_tester::TeamTesterStatus;
use crate::workspaces::update_manager::TeamUpdateManager;
use crate::workspaces::user_workspaces::UserWorkspaces;

/// Initializes all of the necessary models to use a terminal view.
pub fn initialize_app_for_terminal_view(app: &mut App) {
    initialize_history_persistence_for_tests(app);

    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    // Register a disabled `IapManager` (no IAP state) so code paths that read
    // the singleton (e.g. the shared-session viewer network) don't panic in
    // tests. With `None` state it is an inert no-op.
    app.add_singleton_model(|ctx| {
        IapManager::new(
            None,
            Box::new(|_| futures::FutureExt::boxed(futures::future::ready(None::<String>))),
            ctx,
        )
    });
    app.add_singleton_model(|ctx| ChangelogModel::new(ServerApiProvider::as_ref(ctx).get()));
    app.add_singleton_model(|_| NetworkStatus::new());
    app.add_singleton_model(|_| SystemStats::new());
    app.add_singleton_model(|_| Prompt::mock());
    app.add_singleton_model(SyncQueue::mock);
    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(UserWorkspaces::default_mock);
    app.add_singleton_model(TeamTesterStatus::mock);
    app.add_singleton_model(TeamUpdateManager::mock);
    app.add_singleton_model(UpdateManager::mock);
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(PrivacySettings::mock);
    app.add_singleton_model(|_ctx| SyncedInputState::mock());
    app.add_singleton_model(|_| ResizableData::default());
    app.add_singleton_model(LocalWorkflows::new);
    app.add_singleton_model(|_| History::default());
    app.add_singleton_model(UndoCloseStack::new);
    app.add_singleton_model(|_| KeybindingChangedNotifier::new());
    app.add_singleton_model(TerminalKeybindings::new);
    app.add_singleton_model(|_| ActiveSession::default());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(SessionPermissionsManager::new);
    app.add_singleton_model(DirectoryWatcher::new);
    app.add_singleton_model(|_| DetectedRepositories::default());
    #[cfg(feature = "local_fs")]
    app.add_singleton_model(|ctx| {
        let model = RepoMetadataModel::new(ctx);
        model.register_force_included_paths(
            SKILL_PROVIDER_DEFINITIONS
                .iter()
                .map(|provider| provider.skills_path.clone()),
            ctx,
        );
        model.set_project_skill_provider_paths(
            SKILL_PROVIDER_DEFINITIONS
                .iter()
                .map(|provider| provider.skills_path.clone()),
            ctx,
        );
        model
    });
    app.add_singleton_model(FileSearchModel::new);
    app.add_singleton_model(|_| GitRepoModels::new());
    app.add_singleton_model(HomeDirectoryWatcher::new_for_test);
    app.add_singleton_model(WarpManagedPathsWatcher::new_for_testing);
    // LOCAL FORK: the `CodebaseIndexManager` singleton registered here was removed
    // with the codebase indexing surface.
    #[cfg(feature = "voice_input")]
    app.add_singleton_model(voice_input::VoiceInput::new);

    #[cfg(not(target_family = "wasm"))]
    app.add_singleton_model(SystemInfo::new);

    app.add_singleton_model(OneTimeModalModel::new);
    app.add_singleton_model(|_| WorkspaceRegistry::new());
    app.add_singleton_model(|_| IgnoredSuggestionsModel::new(vec![]));
    app.add_singleton_model(|_| PricingInfoModel::new());
    app.add_singleton_model(ByoLlmAuthBannerSessionState::new);
    app.add_singleton_model(PersistedWorkspace::new_for_test);

    app.update(experiments::init);
    AltScreenReporting::register(app);
}

/// Creates a window in `app` with a [`TerminalView`] as the root view.
/// Returns the handle to that terminal view.
pub fn add_window_with_terminal(
    app: &mut App,
    restored_blocks: Option<&[SerializedBlock]>,
) -> ViewHandle<TerminalView> {
    add_window_with_id_and_terminal(app, restored_blocks).1
}

/// Creates a window in `app` with a [`TerminalView`] as the root view.
/// Returns the WindowID and the handle to that terminal view.
pub fn add_window_with_id_and_terminal(
    app: &mut App,
    restored_blocks: Option<&[SerializedBlock]>,
) -> (WindowId, ViewHandle<TerminalView>) {
    // LOCAL FORK: `restored_blocks` used to be `Option<&[SerializedBlockListItem]>`.
    // That single-variant wrapper went with the agent crate; the blocks themselves
    // are plain terminal state, so this carries `SerializedBlock`s directly.
    let tips_model = app.add_model(|_| Default::default());
    app.add_window(WindowStyle::NotStealFocus, |ctx| {
        TerminalView::new_for_test(tips_model, restored_blocks, ctx)
    })
}
