use std::collections::HashSet;
use std::time::Duration;

use chrono::Local;
use fuzzy_match::FuzzyMatchResult;
use repo_metadata::RepoMetadataModel;
use repo_metadata::repositories::DetectedRepositories;
use repo_metadata::watcher::DirectoryWatcher;
use smol_str::SmolStr;
use unindent::Unindent;
use warp_completer::completer::{
    EngineFileType, Match, MatchStrategy, MatchedSuggestion, Priority, Suggestion,
    SuggestionResults, SuggestionType,
};
use warp_completer::meta::Span;
use warp_util::standardized_path::StandardizedPath;
use warp_util::user_input::UserInput;
use warpui::platform::WindowStyle;
use warpui::text::SelectionType;
use warpui::{App, UpdateView, WindowId};
use watcher::HomeDirectoryWatcher;
use workflows::workflow::{Argument, ArgumentType, Workflow};

use super::*;
use crate::auth::AuthStateProvider;
use crate::changelog_model::ChangelogModel;
use crate::cloud_object::model::persistence::CloudModel;
use crate::context_chips::prompt::Prompt;
use crate::editor::{DisplayPoint, EditorAction, Point, TextStyleOperation};
use crate::input_suggestions::Item;
use crate::network::NetworkStatus;
use crate::persisted_workspace::PersistedWorkspace;
use crate::search::files::model::FileSearchModel;
use crate::search::slash_command_menu::static_commands::commands;
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::server_api::ServerApiProvider;
use crate::settings::import::model::ImportedConfigModel;
use crate::settings::{AliasExpansionSettings, AppEditorSettings, PrivacySettings};
use crate::settings_view::keybindings::KeybindingChangedNotifier;
#[cfg(windows)]
use crate::system::SystemInfo;
use crate::system::SystemStats;
use crate::terminal::TerminalView;
use crate::terminal::alt_screen_reporting::AltScreenReporting;
use crate::terminal::block_list_viewport::ScrollPosition;
use crate::terminal::event::{BlockMetadataReceivedEvent, BootstrappedEvent};
use crate::terminal::general_settings::UserDefaultShellUnsupportedBannerState;
use crate::terminal::input::slash_commands::SlashCommandsEvent;
use crate::terminal::keys::TerminalKeybindings;
use crate::terminal::local_shell::LocalShellState;
use crate::terminal::local_tty::shell::ShellStarter;
use crate::terminal::model::ansi::{Handler, PromptMetadata};

use crate::terminal::model::blocks::{BlockListPoint, insert_block};
use crate::terminal::model::grid::Dimensions as _;
use crate::terminal::model::index::Side;
use crate::terminal::model::session::{BootstrapSessionType, SessionInfo};
use crate::terminal::model::terminal_model::BlockIndex;
use crate::terminal::model_events::ModelEvent;
use crate::terminal::resizable_data::ResizableData;
use crate::terminal::shell::ShellType;
use crate::terminal::view::inline_banner::ByoLlmAuthBannerSessionState;
use crate::terminal::writeable_pty::command_history::update_command_history;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::themes::theme::AnsiColorIdentifier;
use crate::warp_managed_paths_watcher::WarpManagedPathsWatcher;
use crate::workspace::{ActiveSession, OneTimeModalModel, ToastStack, WorkspaceRegistry};
use crate::workspaces::team_tester::TeamTesterStatus;
use crate::workspaces::update_manager::TeamUpdateManager;
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::{
    GlobalResourceHandles, GlobalResourceHandlesProvider, ReferralThemeStatus, experiments,
};

#[test]
fn renders_git_checkout_prompt_chip_command_as_single_shell_argument() {
    let command = PromptChipShellCommand::GitCheckout {
        branch_name: "poc;id>/tmp/proof $(whoami) `id` | cat 'tail'".to_string(),
    };

    assert_eq!(
        render_prompt_chip_shell_command(&command, ShellType::Bash),
        r#"git checkout 'poc;id>/tmp/proof $(whoami) `id` | cat '"'"'tail'"'"''"#
    );
    assert_eq!(
        render_prompt_chip_shell_command(&command, ShellType::Zsh),
        r#"git checkout 'poc;id>/tmp/proof $(whoami) `id` | cat '"'"'tail'"'"''"#
    );
    assert_eq!(
        render_prompt_chip_shell_command(&command, ShellType::Fish),
        r"git checkout 'poc;id>/tmp/proof $(whoami) `id` | cat \'tail\''"
    );
    assert_eq!(
        render_prompt_chip_shell_command(&command, ShellType::PowerShell),
        "git checkout 'poc;id>/tmp/proof $(whoami) `id` | cat ''tail'''"
    );
}

#[test]
fn renders_nvm_use_prompt_chip_command_as_single_shell_argument() {
    let command = PromptChipShellCommand::NvmUse {
        version: "v20.0.0;touch /tmp/pwn 'x'".to_string(),
    };

    assert_eq!(
        render_prompt_chip_shell_command(&command, ShellType::Bash),
        r#"nvm use 'v20.0.0;touch /tmp/pwn '"'"'x'"'"''"#
    );
    assert_eq!(
        render_prompt_chip_shell_command(&command, ShellType::Fish),
        r"nvm use 'v20.0.0;touch /tmp/pwn \'x\''"
    );
    assert_eq!(
        render_prompt_chip_shell_command(&command, ShellType::PowerShell),
        "nvm use 'v20.0.0;touch /tmp/pwn ''x'''"
    );
}

#[test]
fn renders_change_directory_prompt_chip_command_as_single_shell_argument() {
    let command = PromptChipShellCommand::ChangeDirectory {
        dir_name: "repo dir;rm -rf / 'x'".to_string(),
    };

    assert_eq!(
        render_prompt_chip_shell_command(&command, ShellType::Bash),
        r#"cd 'repo dir;rm -rf / '"'"'x'"'"''"#
    );
    assert_eq!(
        render_prompt_chip_shell_command(&command, ShellType::PowerShell),
        "cd 'repo dir;rm -rf / ''x'''"
    );
}

#[test]
fn renders_echo_prompt_chip_command_as_single_shell_argument() {
    let command = PromptChipShellCommand::Echo {
        message: "a message containing \"double\" and 'single' quotes",
    };

    assert_eq!(
        render_prompt_chip_shell_command(&command, ShellType::Bash),
        r#"echo 'a message containing "double" and '"'"'single'"'"' quotes'"#
    );
    assert_eq!(
        render_prompt_chip_shell_command(&command, ShellType::PowerShell),
        r#"echo 'a message containing "double" and ''single'' quotes'"#
    );
}

#[test]
fn renders_fixed_prompt_chip_command_without_interpolation() {
    assert_eq!(
        render_prompt_chip_shell_command(
            &PromptChipShellCommand::NvmInstallLatestNode,
            ShellType::Bash,
        ),
        "nvm install node"
    );
}

pub fn initialize_app(app: &mut App) {
    initialize_settings_for_tests(app);

    // NLD is now opt-in by default (`ai_autodetection_enabled_internal` defaults to false).
    // These tests exercise the natural-language-detection-on code paths (buffer-driven slash
    // command detection, auto-detection input mode), so explicitly re-enable it here to preserve
    // the pre-opt-in test behavior. The opt-in default itself is covered by
    // `ai_autodetection_defaults_to_opt_in` in `settings/ai_tests.rs`.
    crate::settings::AISettings::handle(app).update(app, |settings, ctx| {
        settings
            .ai_autodetection_enabled_internal
            .set_value(true, ctx)
            .unwrap();
    });

    // Make sure we set up all necessary custom action bindings.
    app.update(init);

    // Initialize any global models required by the Input view.
    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    app.add_singleton_model(|ctx| ChangelogModel::new(ServerApiProvider::as_ref(ctx).get()));
    app.add_singleton_model(|_| NetworkStatus::new());
    app.add_singleton_model(|_| SystemStats::new());
    app.add_singleton_model(|_| Prompt::mock());
    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(ImportedConfigModel::new);
    app.add_singleton_model(UserWorkspaces::default_mock);
    app.add_singleton_model(TeamTesterStatus::mock);
    app.add_singleton_model(TeamUpdateManager::mock);
    app.add_singleton_model(UpdateManager::mock);
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(PrivacySettings::mock);
    app.add_singleton_model(|_ctx| SyncedInputState::mock());
    app.add_singleton_model(|_| ResizableData::default());
    app.add_singleton_model(|_| History::default());
    app.add_singleton_model(LocalWorkflows::new);
    app.add_singleton_model(|_| KeybindingChangedNotifier::new());
    app.add_singleton_model(TerminalKeybindings::new);
    app.add_singleton_model(|_| ActiveSession::default());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(DirectoryWatcher::new);
    app.add_singleton_model(|_| DetectedRepositories::default());
    app.add_singleton_model(crate::remote_server::manager::RemoteServerManager::new);
    app.add_singleton_model(|_| crate::code_review::git_repo_model::GitRepoModels::new());
    app.add_singleton_model(RepoMetadataModel::new);
    app.add_singleton_model(FileSearchModel::new);
    #[cfg(feature = "voice_input")]
    app.add_singleton_model(voice_input::VoiceInput::new);
    app.add_singleton_model(|_| IgnoredSuggestionsModel::new(vec![]));
    app.add_singleton_model(HomeDirectoryWatcher::new_for_test);
    app.add_singleton_model(WarpManagedPathsWatcher::new_for_testing);

    // Add GlobalResourceHandlesProvider for persistence
    let tips_handle = app.add_model(|_| TipsCompleted::default());
    let referral_theme_status = app.add_model(ReferralThemeStatus::new);
    let user_default_shell_unsupported_banner_model_handle =
        app.add_model(|_| UserDefaultShellUnsupportedBannerState::default_value());
    app.add_singleton_model(move |_ctx| {
        GlobalResourceHandlesProvider::new(GlobalResourceHandles {
            model_event_sender: None, // No persistence in tests
            tips_completed: tips_handle,
            referral_theme_status,
            user_default_shell_unsupported_banner_model_handle,
            settings_file_error: None,
        })
    });

    #[cfg(windows)]
    {
        app.add_singleton_model(SystemInfo::new);
    }

    app.update(experiments::init);
    AltScreenReporting::register(app);
    app.add_singleton_model(OneTimeModalModel::new);
    app.add_singleton_model(|_| WorkspaceRegistry::new());
    app.add_singleton_model(|_| ToastStack);
    app.add_singleton_model(ByoLlmAuthBannerSessionState::new);
    app.add_singleton_model(PersistedWorkspace::new_for_test);
    // `LocalShellState` captures the user's interactive login-shell PATH (used
    // for MCP/sbx executable resolution). Tests don't exercise that capture, so
    // register the singleton in its `NotLoaded` state to satisfy callers that
    // look it up via `LocalShellState::handle(ctx)`.
    app.add_singleton_model(|_| LocalShellState::NotLoaded);
}

fn bootstrap_terminal(
    terminal: &ViewHandle<TerminalView>,
    bootstrapped_event: BootstrappedEvent,
    app: &mut App,
) {
    let session_id = bootstrapped_event.session_info.session_id;
    terminal.update(app, |terminal, ctx| {
        terminal.model.lock().block_list_mut().set_bootstrapped();

        // Set session_id since precmd is not called in unit tests.
        terminal
            .model
            .lock()
            .block_list_mut()
            .active_block_for_test()
            .set_session_id(session_id);
        let model_event_dispatcher = terminal.model_event_dispatcher().clone();
        model_event_dispatcher.update(ctx, |dispatcher, _| {
            dispatcher.set_active_session_id(session_id);
        });

        terminal.sessions_model().update(ctx, |sessions, ctx| {
            let BootstrappedEvent {
                session_info,
                restored_block_commands,
                rcfiles_duration_seconds,
                spawning_command,
            } = bootstrapped_event;
            sessions.initialize_bootstrapped_session(
                *session_info,
                spawning_command,
                restored_block_commands,
                rcfiles_duration_seconds,
                ctx,
            );
        });
    });
}

fn enable_vim_mode(app: &mut App) {
    AppEditorSettings::handle(app).update(app, |editor_settings, ctx| {
        editor_settings
            .vim_mode
            .set_value(true, ctx)
            .expect("set value must succeed");
    });
}

pub async fn add_window_with_bootstrapped_terminal(
    app: &mut App,
    history_file_commands: Option<Vec<String>>,
    session_info: Option<SessionInfo>,
) -> ViewHandle<TerminalView> {
    add_window_with_bootstrapped_terminal_and_window_id(app, history_file_commands, session_info)
        .await
        .1
}

pub async fn add_window_with_bootstrapped_terminal_and_window_id(
    app: &mut App,
    history_file_commands: Option<Vec<String>>,
    session_info: Option<SessionInfo>,
) -> (WindowId, ViewHandle<TerminalView>) {
    let tips_model = app.add_model(|_| TipsCompleted::default());

    let shell_starter_source = ShellStarter::init(Default::default())
        .expect("Could not create a shell starter source or wsl name")
        .to_shell_starter_source()
        .await
        .expect("Could not create a shell starter source");
    let shell_type = shell_starter_source.shell_type();

    let session_info = session_info
        .unwrap_or_else(SessionInfo::new_for_test)
        .with_session_type(BootstrapSessionType::Local)
        .with_shell_type(shell_type);
    let history_file_commands = history_file_commands.unwrap_or_default();

    let (window_id, terminal) = app.add_window(WindowStyle::NotStealFocus, move |ctx| {
        TerminalView::new_for_test(tips_model, None, ctx)
    });

    // TODO(vorporeal): There's a lot of fuckiness here.  `TerminalView::new_for_test`
    // calls `TerminalModel::new_for_test`, which fakes the InitShell and Bootstrapped
    // lifecycle events.  We then _also_ bootstrap the terminal here, which can and does
    // lead to inconsistent states.  We ought to only bootstrap the terminal once.
    let session_id = session_info.session_id;
    let bootstrapped_event = BootstrappedEvent {
        session_info: Box::new(session_info),
        restored_block_commands: history_file_commands
            .into_iter()
            .map(|command| HistoryEntry::command_at_time(command, Local::now(), None, true))
            .collect_vec(),
        rcfiles_duration_seconds: None,
        spawning_command: "test command".to_string(),
    };
    bootstrap_terminal(&terminal, bootstrapped_event, app);

    // Wait until history has been initialized for the session.
    let mut history_handle = History::handle(app);
    History::initialized_sessions(&mut history_handle, app, vec![session_id]).await;

    let input = terminal.read(app, |terminal, _| terminal.input().clone());
    // Notify the input that the session has bootstrapped
    input.update(app, |input, ctx| {
        input.set_active_block_metadata(BlockMetadata::new(Some(session_id), None), false, ctx);
    });
    (window_id, terminal)
}

/// Simulates being in a particular directory, for the purposes of completion
/// and syntax highlighting. The current directory is used to resolve
/// paths when parsing commands, and without it, completion/highlighting will
/// not run.
///
/// In particular, this sends precmd data and sets the active block's metadata.
pub fn simulate_directory_for_completion<A, S>(
    session_id: SessionId,
    terminal: &ViewHandle<TerminalView>,
    app: &mut A,
    directory: S,
) where
    A: UpdateView,
    S: Into<String>,
{
    let directory = directory.into();
    terminal.update(app, |terminal, ctx| {
        let block_metadata = BlockMetadata::new(Some(session_id), Some(directory.clone()));
        let block_index = {
            let mut model = terminal.model.lock();
            model.block_list_mut().prompt_only_precmd(PromptMetadata {
                pwd: Some(directory.clone()),
                session_id: Some(session_id.into()),
                ..Default::default()
            });
            model.block_list().active_block_index()
        };

        // Normally, the precmd message should be sufficient to also set this block metadata.
        // However, in unit tests the foreground executor does not relay the event, so notify
        // the dispatcher directly for models that observe active-session metadata.
        terminal
            .model_event_dispatcher()
            .update(ctx, |dispatcher, ctx| {
                dispatcher.set_active_session_id(session_id);
                ctx.emit(ModelEvent::BlockMetadataReceived(
                    BlockMetadataReceivedEvent {
                        block_metadata: block_metadata.clone(),
                        block_index,
                        is_after_in_band_command: false,
                        is_done_bootstrapping: true,
                    },
                ));
            });

        // Keep the input's block metadata in sync with the active-session metadata above.
        terminal.input().update(ctx, |input, ctx| {
            input.set_active_block_metadata(block_metadata, false, ctx);
        });
    });
}

fn argument_suggestion(name: impl Into<SmolStr>) -> MatchedSuggestion {
    let suggestion = Suggestion::with_same_display_and_replacement(
        name,
        None,
        SuggestionType::Argument,
        Priority::default(),
    );
    MatchedSuggestion::new(
        suggestion,
        Match::Prefix {
            is_case_sensitive: true,
        },
    )
}

/// Creates a [`MatchedSuggestion`] for a file completion result.
/// Specifically, we ensure the replacement is the entire path
/// while the display text is just the string after the last slash.
fn file_suggestion(path: impl Into<SmolStr>) -> MatchedSuggestion {
    let replacement = path.into();
    let display = replacement
        .rsplit(std::path::MAIN_SEPARATOR)
        .next()
        .map(Into::into)
        .unwrap_or_else(|| replacement.clone());

    let suggestion = Suggestion::new(
        display,
        replacement,
        None,
        SuggestionType::Argument,
        Priority::default(),
    )
    .with_file_type(EngineFileType::File);

    MatchedSuggestion::new(
        suggestion,
        Match::Prefix {
            is_case_sensitive: true,
        },
    )
}

fn case_insensitive_argument_suggestion(name: impl Into<SmolStr>) -> MatchedSuggestion {
    let suggestion = Suggestion::with_same_display_and_replacement(
        name,
        None,
        SuggestionType::Argument,
        Priority::default(),
    );
    MatchedSuggestion::new(
        suggestion,
        Match::Prefix {
            is_case_sensitive: false,
        },
    )
}

fn case_insensitive_exact_argument_suggestion(name: impl Into<SmolStr>) -> MatchedSuggestion {
    let suggestion = Suggestion::with_same_display_and_replacement(
        name,
        None,
        SuggestionType::Argument,
        Priority::default(),
    );
    MatchedSuggestion::new(
        suggestion,
        Match::Exact {
            is_case_sensitive: false,
        },
    )
}

fn fuzzy_argument_suggestion(
    name: impl Into<SmolStr>,
    matched_indices: Vec<usize>,
) -> MatchedSuggestion {
    let suggestion = Suggestion::with_same_display_and_replacement(
        name,
        None,
        SuggestionType::Argument,
        Priority::default(),
    );
    MatchedSuggestion::new(
        suggestion,
        Match::Fuzzy {
            match_result: FuzzyMatchResult {
                score: 1,
                matched_indices,
            },
        },
    )
}

fn editor_model_snapshot(input: &Input, ctx: &mut ViewContext<Input>) -> EditorSnapshot {
    input
        .editor()
        .read(ctx, |editor, ctx| editor.snapshot_model(ctx))
}

fn set_alias_expansion_setting(new_value: bool, app: &mut App) {
    AliasExpansionSettings::handle(app).update(app, |settings, ctx| {
        if let Err(e) = settings.alias_expansion_enabled.set_value(new_value, ctx) {
            panic!("Unable to set alias expansion setting in test, {e:?}");
        }
    });
}

/// Inserts block with dummy text and returns the block index.
fn insert_dummy_block(terminal: ViewHandle<TerminalView>, app: &mut App) -> BlockIndex {
    terminal.update(app, |terminal_view, _ctx| {
        let mut terminal_model = terminal_view.model.lock();
        let blocks = terminal_model.block_list_mut();
        // Add two lines to the command grid and output grid in a new block.
        insert_block(blocks, "cmd_a\ncmd_b\n", "output_a\noutput_b\n")
    })
}

/// Selects the first line in the command grid of given block.
fn select_first_command_line_of_block(
    block_index: BlockIndex,
    terminal: ViewHandle<TerminalView>,
    app: &mut App,
) {
    terminal.update(app, |terminal_view, _ctx| {
        let mut terminal_model = terminal_view.model.lock();
        let blocks = terminal_model.block_list_mut();
        let block = blocks.block_at(block_index).expect("block should exist");
        // Selections are inclusive of endpoint, hence we need to identify the last column to select the first command.
        let block_command_columns = block.prompt_and_command_grid().grid_handler().columns();
        let command_grid_offset = block.command_grid_offset();
        // Create a selection that just spans the first line of the command grid in the block.
        blocks.start_selection(
            BlockListPoint::new(command_grid_offset, 0),
            SelectionType::Simple,
            Side::Left,
        );
        blocks.update_selection(
            BlockListPoint::new(command_grid_offset, block_command_columns),
            Side::Right,
        );
        let selection = blocks.selection();
        assert!(selection.is_some());
    });
}

#[test]
fn test_input_tab() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        // Note: we have similar boilerplate for many tests in this file - it would be nice to refactor this into a common helper!
        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let editor = input.read(&app, |input, _| input.editor().clone());
        // If there is no non-whitespace input, pass the tab to the editor
        input.read(&app, |input, ctx| {
            assert!(input.buffer_text(ctx).is_empty());
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "    ");
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "        ");
        });
        input.update(&mut app, |input, ctx| {
            input.input_shift_tab(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "    ");
        });

        // Test that if there is a single cursor at the end, we do not pass tab to the editor.
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("c", ctx);
            input.user_insert("d", ctx);
            input.user_insert(" ", ctx);
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd ");
        });

        // Test that we don't pass the tab if the single cursor is in the middle either
        input.update(&mut app, |input, ctx| {
            input.user_insert("s", ctx);
            input.user_insert("o", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_left(/* stop at line start */ false, ctx);
            editor.move_left(/* stop at line start */ false, ctx);
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd so");
        });

        // Test that if we select the entire buffer, we pass tab to the editor.
        input.update(&mut app, |input, ctx| {
            input.editor.update(ctx, |editor, ctx| {
                editor.select_all(ctx);
            })
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "    cd so");
        });
    });
}

#[test]
fn zero_state_hint_text_only_registers_active_slash_command_placeholders() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let session_info = SessionInfo::new_for_test();
        let session_id = session_info.session_id;
        let terminal = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.set_zero_state_hint_text(ctx);
        });

        let editor = input.read(&app, |input, _| input.editor().clone());
        let rename_tab_prefix = format!("{} ", commands::RENAME_TAB.name);
        let continue_locally_prefix = format!("{} ", commands::CONTINUE_LOCALLY.name);

        editor.update(&mut app, |editor, ctx| {
            editor.set_placeholder_text_with_prefix(
                continue_locally_prefix.clone(),
                "stale hint",
                ctx,
            );
        });
        input.update(&mut app, |input, ctx| {
            input.set_zero_state_hint_text(ctx);
        });

        assert!(
            editor.read(&app, |editor, _| editor
                .placeholder_text(&rename_tab_prefix)
                .is_some()),
            "always-active slash command placeholders should still be registered"
        );
        assert!(
            editor.read(&app, |editor, _| editor
                .placeholder_text(&continue_locally_prefix)
                .is_none()),
            "/continue-locally should not be registered outside cloud conversation context"
        );

        editor.update(&mut app, |editor, ctx| {
            editor.set_placeholder_text_with_prefix(
                continue_locally_prefix.clone(),
                "stale hint",
                ctx,
            );
        });

        let repo_dir = tempfile::TempDir::new().expect("repo temp dir");
        let repo_path = repo_dir.path().to_path_buf();
        simulate_directory_for_completion(
            session_id,
            &terminal,
            &mut app,
            repo_path.to_string_lossy().into_owned(),
        );
        DetectedRepositories::handle(&app).update(&mut app, |repos, _| {
            let root = StandardizedPath::from_local_canonicalized(&repo_path)
                .expect("canonicalized repo root");
            repos.insert_test_repo_root(root);
        });
        input.update(&mut app, |input, ctx| {
            input.update_repo_path(Some(repo_path), ctx);
        });

        assert!(
            editor.read(&app, |editor, _| editor
                .placeholder_text(&continue_locally_prefix)
                .is_none()),
            "active slash-command data source updates should refresh stale placeholders"
        );
    });
}

#[test]
fn test_clear_selection_after_insert() {
    // We should clear the selection after inserting text into the input box (both
    // user-inserted and system-inserted text).
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let session_info = SessionInfo::new_for_test();
        let terminal: ViewHandle<TerminalView> = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;
        let input = terminal.read(&app, |terminal, _ctx| terminal.input().clone());
        input.update(&mut app, |input, ctx| {
            input.set_active_block_metadata(
                BlockMetadata::new(Some(SessionId::from(0)), Some("~".into())),
                false,
                ctx,
            )
        });

        let select_text = |app: &mut App| {
            let block_index = insert_dummy_block(terminal.clone(), app);
            select_first_command_line_of_block(block_index, terminal.clone(), app);
        };
        let user_insert = |app: &mut App, text: &str| {
            input.update(app, |input, ctx| {
                input.user_insert(text, ctx);
            });
        };
        let assert_selections_in_blocklist = |app: &mut App, expect_selections: bool| {
            terminal.read(app, |terminal_view, _ctx| {
                let terminal_model = terminal_view.model.lock();
                let blocks = terminal_model.block_list();
                let selection = blocks.selection();
                assert_eq!(selection.is_some(), expect_selections);
            });
        };

        // Shell Mode: Insert some text into the input box - this should clear the terminal selection!
        select_text(&mut app);
        user_insert(&mut app, "bar");
        assert_selections_in_blocklist(&mut app, false);

        // Shell Mode: System insert should also clear terminal selection.
        select_text(&mut app);
        user_insert(&mut app, "baz");
        assert_selections_in_blocklist(&mut app, false);
    });
}

#[test]
fn test_command_history_ordering() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let now = Local::now();
        let current_session_id = SessionId::from(0);
        let other_session_id = SessionId::from(1);
        let all_live_session_ids = HashSet::from([current_session_id, other_session_id]);

        // Create entries in chronological order (from earliest to most recent)
        // Restored commands are now treated as CurrentSession
        let entry_30s = HistoryEntry::command_at_time(
            "echo 30 sec earlier [restored]".into(),
            now - Duration::from_secs(30),
            Some(current_session_id),
            true,
        );
        let entry_20s = HistoryEntry::command_at_time(
            "echo 20 sec earlier [different session]".into(),
            now - Duration::from_secs(20),
            None,
            false,
        );
        let entry_10s = HistoryEntry::command_at_time(
            "echo 10 sec earlier [current session]".into(),
            now - Duration::from_secs(10),
            Some(current_session_id),
            false,
        );
        let entry_5s = HistoryEntry::command_at_time(
            "echo 5 sec earlier [other session]".into(),
            now - Duration::from_secs(5),
            Some(other_session_id),
            false,
        );
        let entry_now =
            HistoryEntry::command_at_time("echo now [different session]".into(), now, None, false);

        let history_commands = vec![
            HistoryInputSuggestion::Command { entry: &entry_20s },
            HistoryInputSuggestion::Command { entry: &entry_now },
            HistoryInputSuggestion::Command { entry: &entry_30s },
            HistoryInputSuggestion::Command { entry: &entry_10s },
            HistoryInputSuggestion::Command { entry: &entry_5s },
        ];
        let only_history_commands = history_commands
            .clone()
            .into_iter()
            .sorted_by(|a, b| a.cmp(b, Some(current_session_id), &all_live_session_ids))
            .collect::<Vec<_>>();
        assert_eq!(only_history_commands.len(), 5);
        // DifferentSession items sorted by timestamp
        assert_eq!(
            only_history_commands[0].text(),
            "echo 20 sec earlier [different session]"
        );
        assert_eq!(
            only_history_commands[1].text(),
            "echo 5 sec earlier [other session]"
        );
        assert_eq!(
            only_history_commands[2].text(),
            "echo now [different session]"
        );
        // CurrentSession items sorted by timestamp (restored + current session)
        assert_eq!(
            only_history_commands[3].text(),
            "echo 30 sec earlier [restored]"
        );
        assert_eq!(
            only_history_commands[4].text(),
            "echo 10 sec earlier [current session]"
        );
    });
}

#[test]
fn test_history_up() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let history_file_commands = vec![
            "cd /".to_string(),
            "cd ~".to_string(),
            "git add .".to_string(),
            "ls cd".to_string(),
        ];
        let terminal =
            add_window_with_bootstrapped_terminal(&mut app, Some(history_file_commands), None)
                .await;
        let (input, editor, suggestions) = terminal.read(&app, |view, ctx| {
            let input = view.input().clone();
            let editor = input.as_ref(ctx).editor().clone();
            let input_suggestions = input.read(&app, |input, _ctx| input.input_suggestions.clone());
            (input, editor, input_suggestions)
        });

        // Arrow up displays history in the correct order for an empty buffer
        input.update(&mut app, |input, ctx| {
            input.editor_up(ctx);
        });
        suggestions.read(&app, |suggestions, _ctx| {
            assert_eq!(suggestions.items().len(), 4);
            assert_eq!(suggestions.item_text(0).as_str(), "cd /");
            assert_eq!(suggestions.item_text(1).as_str(), "cd ~");
            assert_eq!(suggestions.item_text(2).as_str(), "git add .");
            assert_eq!(suggestions.item_text(3).as_str(), "ls cd");
        });

        // The buffer should contain the text of the last item
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "ls cd");
        });

        // The buffer contain the text of the second last item after another arrow-up
        input.update(&mut app, |input, ctx| {
            input.editor_up(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "git add .");
        });

        // Now put some text into the input and assert it has ctrl-r behavior on
        // arrow up
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("c", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "c");
        });
        input.update(&mut app, |input, ctx| {
            input.editor_up(ctx);
        });
        suggestions.read(&app, |suggestions, _ctx| {
            // Shouldn't contain the "ls cd"
            assert_eq!(suggestions.items().len(), 2);
            assert_eq!(suggestions.item_text(0).as_str(), "cd /");
            assert_eq!(suggestions.item_text(1).as_str(), "cd ~");
        });

        // The buffer should contain the text of the last item
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd ~");
        });

        // The buffer contain the text of the second last item after another arrow-up
        input.update(&mut app, |input, ctx| {
            input.editor_up(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd /");
        });

        // Another editor-up is a no-op
        input.update(&mut app, |input, ctx| {
            input.editor_up(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd /");
        });

        // Closing the history up has left the buffer unchanged
        input.update(&mut app, |input, ctx| {
            input.editor_escape(ctx);
        });
        input.read(&app, |input, ctx| {
            assert!(input.suggestions_mode_model.as_ref(ctx).is_closed());
            assert_eq!(input.buffer_text(ctx), "c");
        });
        editor.read(&app, |editor, ctx| {
            assert!(
                editor.single_cursor_on_first_row(ctx),
                "Should be single cursor on first row"
            );
        });

        // Test closing the history up menu again with the cursor in the
        // middle of the buffer.
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("foo bar", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            for _ in 0..4 {
                editor.move_left(/* stop at line start */ false, ctx);
            }
        });
        editor.read(&app, |editor, ctx| {
            assert!(
                editor.single_cursor_on_first_row(ctx),
                "Should be single cursor on first row"
            );
            assert_eq!(
                editor.single_cursor_to_point(ctx).unwrap(),
                Point { row: 0, column: 3 },
            );
        });
        input.update(&mut app, |input, ctx| {
            input.editor_up(ctx);
        });
        input.read(&app, |input, ctx| {
            assert!(
                input.suggestions_mode_model.as_ref(ctx).is_visible(),
                "Input suggestions should be visible",
            );
        });
        suggestions.read(&app, |suggestions, _ctx| {
            assert!(suggestions.items().is_empty());
        });
        input.update(&mut app, |input, ctx| {
            // This time use editor down to close the menu
            input.editor_down(ctx);
        });
        input.read(&app, |input, ctx| {
            assert!(
                !input.suggestions_mode_model.as_ref(ctx).is_visible(),
                "Input suggestions should be dismissed",
            );
        });
        editor.read(&app, |editor, ctx| {
            assert_eq!(
                editor.single_cursor_to_point(ctx).unwrap(),
                Point { row: 0, column: 3 },
            );
        });
    });
}

#[test]
fn test_history_up_buffer_restoration() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let history_file_commands = vec![
            "cd /".to_string(),
            "cd ~".to_string(),
            "git add .".to_string(),
            "ls cd".to_string(),
        ];
        let terminal =
            add_window_with_bootstrapped_terminal(&mut app, Some(history_file_commands), None)
                .await;
        let (input, suggestions) = terminal.read(&app, |view, _| {
            let input = view.input().clone();
            let input_suggestions = input.read(&app, |input, _ctx| input.input_suggestions.clone());
            (input, input_suggestions)
        });

        // Arrow up displays history in the correct order for an empty buffer
        input.update(&mut app, |input, ctx| {
            input.editor_up(ctx);
        });
        suggestions.read(&app, |suggestions, _ctx| {
            assert_eq!(suggestions.items().len(), 4);
            assert_eq!(suggestions.item_text(0).as_str(), "cd /");
            assert_eq!(suggestions.item_text(1).as_str(), "cd ~");
            assert_eq!(suggestions.item_text(2).as_str(), "git add .");
            assert_eq!(suggestions.item_text(3).as_str(), "ls cd");
        });
        // The buffer should contain the text of the last item
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "ls cd");
        });

        // should_restore_buffer_before_history_up is true, so our buffer should go back to empty string.
        suggestions.update(&mut app, |suggestions, ctx| {
            suggestions.exit(true, ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "");
        });

        // History up again to the first history entry.
        input.update(&mut app, |input, ctx| {
            input.editor_up(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "ls cd");
        });

        // should_restore_buffer_before_history_up is false, so our buffer should remain unchanged.
        suggestions.update(&mut app, |suggestions, ctx| {
            suggestions.exit(false, ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "ls cd");
        });
    });
}

#[test]
fn test_history_up_multiline() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let history_file_commands = vec![
            "cd ~\necho hello".to_string(),
            "git add .\n git rm .".to_string(),
        ];
        let terminal =
            add_window_with_bootstrapped_terminal(&mut app, Some(history_file_commands), None)
                .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let suggestions = input.read(&app, |input, _ctx| input.input_suggestions.clone());

        input.update(&mut app, |input, ctx| {
            input.editor_up(ctx);
        });
        suggestions.read(&app, |suggestions, _ctx| {
            assert_eq!(suggestions.items().len(), 2);
            assert_eq!(suggestions.item_text(1).as_str(), "git add .\n git rm .");
            assert_eq!(suggestions.item_text(0).as_str(), "cd ~\necho hello");
        });
        input.read(&app, |input, ctx| {
            assert_eq!("git add .\n git rm .", input.buffer_text(ctx));
        });
        input.update(&mut app, |input, ctx| {
            input.editor_up(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!("cd ~\necho hello", input.buffer_text(ctx));
        });
        // Closing the history up menu restores the original buffer
        input.update(&mut app, |input, ctx| {
            input.editor_escape(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                &InputSuggestionsMode::Closed
            );
            assert!(input.buffer_text(ctx).is_empty());
        });
    });
}

#[test]
fn test_history_up_multiline_vim() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let history_file_commands = vec![
            "cd ~\necho hello".to_string(),
            "git add .\n git rm .".to_string(),
        ];

        // Create a terminal window with Vim mode enbled.
        let terminal =
            add_window_with_bootstrapped_terminal(&mut app, Some(history_file_commands), None)
                .await;
        let input = &terminal.read(&app, |terminal, _| terminal.input().clone());
        let suggestions = input.read(&app, |input, _ctx| input.input_suggestions.clone());
        let editor = input.read(&app, |input, _ctx| input.editor.clone());
        AppEditorSettings::handle(&app).update(&mut app, |settings, settings_ctx| {
            let _ = settings.vim_mode.set_value(true, settings_ctx);
        });

        // Switch into Vim Normal mode.
        editor.update(&mut app, |editor, ctx| {
            editor.vim_keystroke(&Keystroke::parse("escape").unwrap(), ctx);
        });
        editor.read(&app, |editor, ctx| {
            assert_eq!(editor.vim_mode(ctx), Some(VimMode::Normal));
        });

        let vim_up_action = EditorAction::VimUserInsert(UserInput::new("k"));
        let vim_down_action = EditorAction::VimUserInsert(UserInput::new("j"));

        // Trigger the history menu.
        input.update(&mut app, |input, ctx| {
            input.handle_action(&InputAction::Up, ctx);
        });

        // The first suggestion should be inserted into the input buffer.
        suggestions.read(&app, |suggestions, _ctx| {
            assert_eq!(suggestions.items().len(), 2);
            assert_eq!(suggestions.item_text(1).as_str(), "git add .\n git rm .");
            assert_eq!(suggestions.item_text(0).as_str(), "cd ~\necho hello");
        });
        input.read(&app, |input, ctx| {
            assert_eq!("git add .\n git rm .", input.buffer_text(ctx));
        });

        // Move up within the input buffer.
        editor.update(&mut app, |editor, ctx| {
            editor.handle_action(&vim_up_action, ctx);
        });

        // The contents of the buffer should not change
        // because the cursor moved up one line.
        input.read(&app, |input, ctx| {
            assert_eq!("git add .\n git rm .", input.buffer_text(ctx));
        });

        // Attempt to move up from the first line in the input buffer.
        editor.update(&mut app, |editor, ctx| {
            editor.handle_action(&vim_up_action, ctx);
        });

        // Now that we've reached the first line,
        // the upward motion takes us to the next suggestion.
        input.read(&app, |input, ctx| {
            assert_eq!("cd ~\necho hello", input.buffer_text(ctx));
        });

        // Move down from the bottom line of the second suggestion.
        editor.update(&mut app, |editor, ctx| {
            editor.handle_action(&vim_down_action, ctx);
        });

        // Since the cursor was on the bottom line,
        // We now go back on the last suggestion.
        input.read(&app, |input, ctx| {
            assert_eq!("git add .\n git rm .", input.buffer_text(ctx));
        });

        // Move down from the bottom line of the last suggestion.
        editor.update(&mut app, |editor, ctx| {
            editor.handle_action(&vim_down_action, ctx);
        });

        // Now that we've reached the last line,
        // This closes the history up menu and restores the original buffer.
        input.read(&app, |input, ctx| {
            assert_eq!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                &InputSuggestionsMode::Closed
            );
            assert!(input.buffer_text(ctx).is_empty());
        });
    });
}

/// TODO(andy) This test depends on [`terminal::writeable_pty::command_history::update_command_history`]
/// It should be moved into its own test module there, as that is really what's being tested here,
/// i.e. that is where the check for ignorespace is actually happening. I left it here due to the
/// complexity of setting up that test. As that module depends on a TerminalModel with a valid
/// BlockList, it was easier to utilize the boilerplate local to this module. Long-term, some of
/// these helpers should move into shared test utils to make setup easier.
#[cfg_attr(windows, ignore = "TODO(CORE-3626)")]
#[test]
fn test_histignorespace_support_in_zsh() {
    let session_id: SessionId = 1.into();
    let session_info = SessionInfo::new_for_test()
        .with_id(session_id)
        .with_shell_type(ShellType::Zsh)
        .with_shell_options(HashSet::from(["histignorespace".into()]));

    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;

        // Ensure history is in a known (empty) state.
        History::handle(&app).read(&app, |history, _ctx| {
            assert!(history.commands(session_id).unwrap().is_empty());
        });

        // Run "cd" to populate the history buffer.
        let input = terminal.read(&app, |view, _| view.input().clone());
        input.update(&mut app, |input, ctx| {
            input.try_execute_command("cd", ctx);
        });

        // Run "ls" with a leading space, which should prevent history insertion.
        input.update(&mut app, |input, ctx| {
            input.try_execute_command(" ls", ctx);
        });

        let (model, sessions) = terminal.read(&app, |terminal, _| {
            (terminal.model.clone(), terminal.sessions_model().clone())
        });

        app.update(|ctx| {
            update_command_history(
                &ExecuteCommandEvent {
                    command: "cd".into(),
                    session_id,
                    workflow_id: None,
                    workflow_command: None,
                    should_add_command_to_history: true,
                    source: CommandExecutionSource::User,
                },
                &model,
                None,
                &sessions,
                ctx,
            );

            update_command_history(
                &ExecuteCommandEvent {
                    command: " ls".into(),
                    session_id,
                    workflow_id: None,
                    workflow_command: None,
                    should_add_command_to_history: true,
                    source: CommandExecutionSource::User,
                },
                &model,
                None,
                &sessions,
                ctx,
            );
        });

        // Verify only "cd" made it into history.
        History::handle(&app).read(&app, |history, _ctx| {
            assert_eq!(
                history
                    .commands(session_id)
                    .unwrap()
                    .into_iter()
                    .map(|entry| entry.command.as_str())
                    .collect_vec(),
                vec!["cd"]
            );
        });
    });
}

fn build_suggestion_results<S: Into<Span>>(
    suggestions: Vec<MatchedSuggestion>,
    replacement_span: S,
    matcher: MatchStrategy,
) -> Option<SuggestionResults> {
    Some(SuggestionResults {
        replacement_span: replacement_span.into(),
        suggestions,
        match_strategy: matcher,
    })
}

#[test]
fn test_tab_completion_with_multibyte_chars() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |view, _| view.input().clone());

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("➤", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "➤");
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "➤");
        });
    });
}

#[test]
fn test_tab_completion_with_cursor_movement() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let session_info = SessionInfo::new_for_test();
        let session_id = session_info.session_id;
        let terminal = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;
        // Simulate being in the /usr/bin directory.
        simulate_directory_for_completion(session_id, &terminal, &mut app, "/usr/bin");
        let input = terminal.read(&app, |view, _| view.input().clone());

        // Start the editor with the text "yarn a" and press tab to ensure tab completions are
        // showing.
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("yarn a", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "yarn a");
        });
        input.update(&mut app, |input, ctx| {
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("add"),
                        argument_suggestion("audit"),
                        argument_suggestion("autoclean"),
                    ],
                    (5, 5),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            )
            // Somehow `completion_session_context` is yielding None for pwd
        });
        input.read(&app, |input, ctx| {
            // Tab completion menu should be open.
            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::CompletionSuggestions { .. }
            ))
        });

        input.read(&app, |input, _ctx| {
            input
                .input_suggestions
                .read(&app, |input_suggestions, _ctx| {
                    assert!(
                        input_suggestions
                            .items()
                            .iter()
                            .map(|item| item.text())
                            .eq(["add", "audit", "autoclean",])
                    )
                });
        });

        // Add a character and ensure items are filtered down.
        input.update(&mut app, |input, ctx| {
            input.user_insert("u", ctx);
        });

        input.read(&app, |input, ctx| {
            input
                .input_suggestions
                .read(&app, |input_suggestions, _ctx| {
                    assert!(
                        input_suggestions
                            .items()
                            .iter()
                            .map(|item| item.text())
                            .eq(["audit", "autoclean",])
                    )
                });

            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::CompletionSuggestions { .. }
            ))
        });

        // Move cursor to the left--all the results should now appear.
        input.update(&mut app, |input, ctx| {
            input.editor.update(ctx, |editor, ctx| {
                editor.move_left(/* stop at line start */ false, ctx);
            })
        });

        input.read(&app, |input, ctx| {
            input
                .input_suggestions
                .read(&app, |input_suggestions, _ctx| {
                    assert!(
                        input_suggestions
                            .items()
                            .iter()
                            .map(|item| item.text())
                            .eq(["add", "audit", "autoclean",])
                    )
                });

            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::CompletionSuggestions { .. }
            ))
        });

        // Move cursor to the left one more time, the input suggestions menu should be closed.
        input.update(&mut app, |input, ctx| {
            input.editor.update(ctx, |editor, ctx| {
                editor.move_left(/* stop at line start */ false, ctx);
            })
        });

        input.read(&app, |input, ctx| {
            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::Closed
            ))
        });
    });
}

#[test]
fn test_tab_completion_with_leading_space() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |view, _| view.input().clone());
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert(" cd asdf", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), " cd asdf");
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), " cd asdf");
        });
    });
}

#[test]
fn test_tab_completion_with_spaces() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let history_file_commands = vec![
            "cd Documents/zed".to_string(),
            "curl https://app.warp.dev".to_string(),
            "cargo check\ncargo run".to_string(),
        ];
        let terminal =
            add_window_with_bootstrapped_terminal(&mut app, Some(history_file_commands), None)
                .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let (editor, suggestions) = input.read(&app, |input, _| {
            let editor = input.editor().clone();
            let input_suggestions = input.input_suggestions.clone();
            (editor, input_suggestions)
        });

        // Single result tab completion should update buffer.
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("cd A\\ p", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd A\\ p");
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![argument_suggestion("A\\ path\\ with\\ spaces")],
                    (3, 7),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd A\\ path\\ with\\ spaces ");
        });

        // Multiple result tab completion should show menu and highlight the matches.
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("cd A\\ ", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd A\\ ");
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("A\\ dir\\ with\\ spaces"),
                        argument_suggestion("A\\ desktop"),
                    ],
                    (3, 6),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        // We should be highlighting the prefix matches from the last word.
        suggestions.read(&app, |suggestions, _| {
            let highlights = suggestions
                .items()
                .iter()
                .map(|item| item.matches())
                .collect::<Vec<_>>();
            assert_eq!(
                highlights,
                [
                    Some(&(0..4).collect::<Vec<_>>()),
                    Some(&(0..4).collect::<Vec<_>>())
                ]
            );
        });

        suggestions.update(&mut app, |suggestions, ctx| {
            suggestions.select_next(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd A\\ d");
        });

        // Closing the input suggestions menu leaves input buffer unchanged,
        // regardless of whether additional characters were inserted/removed from the original completion buffer text.
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd A\\ d");
        });
        suggestions.update(&mut app, |suggestions, ctx| {
            suggestions.exit(true, ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(
                *input.suggestions_mode_model().as_ref(ctx).mode(),
                InputSuggestionsMode::Closed
            );
            assert_eq!(input.buffer_text(ctx), "cd A\\ d");
        });

        // Inserting a character prefix-searches previous results.
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("A\\ dir\\ with\\ spaces"),
                        argument_suggestion("A\\ desktop"),
                    ],
                    (3, 7),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
            input.user_insert("e", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd A\\ de");
        });
        suggestions.read(&app, |suggestions, _ctx| {
            assert_eq!(suggestions.items().len(), 1);
            assert_eq!(suggestions.item_text(0), "A\\ desktop");
            let highlight = suggestions.items()[0].matches();
            assert_eq!(highlight, Some(&(0..5).collect::<Vec<_>>()));
        });

        // Typing out an entire suggestion should highlight the entire suggestion.
        input.update(&mut app, |input, ctx| {
            input.user_insert("sktop", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd A\\ desktop");
        });
        suggestions.read(&app, |suggestions, _ctx| {
            assert_eq!(suggestions.items().len(), 1);
            assert_eq!(suggestions.item_text(0), "A\\ desktop");
            let highlight = suggestions.items()[0].matches();
            assert_eq!(highlight, Some(&(0..10).collect::<Vec<_>>()));
        });

        // Deleting a character that wasn't part of the original completion buffer updates suggestions.
        editor.update(&mut app, |editor, ctx| {
            for _ in 0.."esktop".len() {
                editor.backspace(ctx);
            }
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd A\\ d");
            assert_ne!(
                *input.suggestions_mode_model().as_ref(ctx).mode(),
                InputSuggestionsMode::Closed
            );
        });
        suggestions.read(&app, |suggestions, _ctx| {
            assert_eq!(suggestions.items().len(), 2);
            assert_eq!(suggestions.item_text(1), "A\\ desktop");
            assert_eq!(suggestions.item_text(0), "A\\ dir\\ with\\ spaces");
            let highlights = suggestions
                .items()
                .iter()
                .map(|item| item.matches())
                .collect::<Vec<_>>();
            assert_eq!(
                highlights,
                [
                    Some(&(0..4).collect::<Vec<_>>()),
                    Some(&(0..4).collect::<Vec<_>>())
                ]
            );
        });

        // Deleting a character that was part of the original completion buffer closes the suggestions menu
        editor.update(&mut app, |editor, ctx| editor.backspace(ctx));
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd A\\ ");
            assert_eq!(
                *input.suggestions_mode_model().as_ref(ctx).mode(),
                InputSuggestionsMode::Closed
            );
        });

        // Bring up suggestions one more time
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("A\\ dir\\ with\\ spaces"),
                        argument_suggestion("A\\ desktop"),
                    ],
                    (3, 6),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });

        // Use tab to select next element, tab-shift to go to the previous & enter to confirm
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
        });
        input.read(&app, |input, _| {
            // after first tab
            input.input_suggestions.read(&app, |suggestions, _| {
                assert_eq!(suggestions.get_selected_item_text().unwrap(), "A\\ desktop");
            });
        });
        input.update(&mut app, |input, ctx| {
            input.input_shift_tab(ctx);
            input.input_enter(ctx);
        });
        input.read(&app, |input, ctx| {
            // shift-tab, enter
            assert_eq!(input.buffer_text(ctx), "cd A\\ dir\\ with\\ spaces ");
            assert_eq!(
                *input.suggestions_mode_model().as_ref(ctx).mode(),
                InputSuggestionsMode::Closed
            );
        });
    });
}

#[test]
fn test_tab_completion() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let history_file_commands = vec![
            "cd Documents/zed".to_string(),
            "curl https://app.warp.dev".to_string(),
            "cargo check\ncargo run".to_string(),
        ];
        let terminal =
            add_window_with_bootstrapped_terminal(&mut app, Some(history_file_commands), None)
                .await;

        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let (editor, suggestions) = input.read(&app, |input, _| {
            let editor = input.editor().clone();
            let input_suggestions = input.input_suggestions.clone();
            (editor, input_suggestions)
        });

        // Single result tab completion should update buffer.
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("c", ctx);
            input.user_insert("d", ctx);
            input.user_insert(" ", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd ");
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![argument_suggestion("Documents")],
                    (3, 3),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd Documents ");
        });

        // Multiple result tab completion should show menu and highlight the matches.
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("c", ctx);
            input.user_insert("d", ctx);
            input.user_insert(" ", ctx);
            input.user_insert("D", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd D");
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("Downloads"),
                        argument_suggestion("Desktop"),
                    ],
                    (3, 4),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        // We should be highlighting the prefix matches from the last word.
        suggestions.read(&app, |suggestions, _| {
            let highlights = suggestions
                .items()
                .iter()
                .map(|item| item.matches())
                .collect::<Vec<_>>();
            assert_eq!(
                highlights,
                [
                    Some(&(0..1).collect::<Vec<_>>()),
                    Some(&(0..1).collect::<Vec<_>>())
                ]
            );
        });

        suggestions.update(&mut app, |suggestions, ctx| {
            suggestions.select_next(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd D");
        });

        // Closing the input suggestions menu leaves input buffer unchanged,
        // regardless of whether additional characters were inserted/removed from the original completion buffer text.
        input.update(&mut app, |input, ctx| {
            input.user_insert("o", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd Do");
        });
        suggestions.update(&mut app, |suggestions, ctx| {
            suggestions.exit(true, ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(
                *input.suggestions_mode_model().as_ref(ctx).mode(),
                InputSuggestionsMode::Closed
            );
            assert_eq!(input.buffer_text(ctx), "cd Do");
        });

        // Inserting a character prefix-searches previous results.
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("Downloads"),
                        argument_suggestion("Documents"),
                    ],
                    (3, 5),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
            input.user_insert("c", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd Doc");
        });
        suggestions.read(&app, |suggestions, _ctx| {
            assert_eq!(suggestions.items().len(), 1);
            assert_eq!(suggestions.item_text(0), "Documents");
            let highlight = suggestions.items()[0].matches();
            assert_eq!(highlight, Some(&(0..3).collect::<Vec<_>>()));
        });

        // Typing out an entire suggestion should highlight the entire suggestion.
        input.update(&mut app, |input, ctx| {
            input.user_insert("uments", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd Documents");
        });
        suggestions.read(&app, |suggestions, _ctx| {
            assert_eq!(suggestions.items().len(), 1);
            assert_eq!(suggestions.item_text(0), "Documents");
            let highlight = suggestions.items()[0].matches();
            assert_eq!(highlight, Some(&(0..9).collect::<Vec<_>>()));
        });

        // Deleting a character that wasn't part of the original completion buffer updates suggestions.
        editor.update(&mut app, |editor, ctx| {
            for _ in 0.."cuments".len() {
                editor.backspace(ctx);
            }
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd Do");
            assert_ne!(
                *input.suggestions_mode_model().as_ref(ctx).mode(),
                InputSuggestionsMode::Closed
            );
        });
        suggestions.read(&app, |suggestions, _ctx| {
            assert_eq!(suggestions.items().len(), 2);
            assert_eq!(suggestions.item_text(1), "Documents");
            assert_eq!(suggestions.item_text(0), "Downloads");
            let highlights = suggestions
                .items()
                .iter()
                .map(|item| item.matches())
                .collect::<Vec<_>>();
            assert_eq!(
                highlights,
                [
                    Some(&(0..2).collect::<Vec<_>>()),
                    Some(&(0..2).collect::<Vec<_>>())
                ]
            );
        });

        // Deleting a character that was part of the original completion buffer closes the suggestions menu
        editor.update(&mut app, |editor, ctx| editor.backspace(ctx));
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd D");
            assert_eq!(
                *input.suggestions_mode_model().as_ref(ctx).mode(),
                InputSuggestionsMode::Closed
            );
        });

        // Bring up suggestions one more time
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("Desktop"),
                        argument_suggestion("Downloads"),
                        argument_suggestion("Documents"),
                    ],
                    (3, 4),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });

        // Use tab to select next element, tab-shift to go to the previous & enter to confirm
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
        });
        input.read(&app, |input, _| {
            // after first tab
            input.input_suggestions.read(&app, |suggestions, _| {
                assert_eq!(suggestions.get_selected_item_text().unwrap(), "Downloads");
            });
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
        });
        input.read(&app, |input, _| {
            // second tab
            input.input_suggestions.read(&app, |suggestions, _| {
                assert_eq!(suggestions.get_selected_item_text().unwrap(), "Documents");
            });
        });
        input.update(&mut app, |input, ctx| {
            input.input_shift_tab(ctx);
            input.input_enter(ctx);
        });
        input.read(&app, |input, ctx| {
            // shift-tab, enter
            // Accepting a suggestion inserts a space at the end
            assert_eq!(input.buffer_text(ctx), "cd Downloads ");
            assert_eq!(
                *input.suggestions_mode_model().as_ref(ctx).mode(),
                InputSuggestionsMode::Closed
            );
        });
    });
}

#[cfg_attr(windows, ignore = "TODO(CORE-3626)")]
#[test]
fn test_tab_completion_with_selection() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let history_file_commands = vec![
            "cd Documents/zed".to_string(),
            "curl https://app.warp.dev".to_string(),
            "cargo check\ncargo run".to_string(),
        ];
        let terminal =
            add_window_with_bootstrapped_terminal(&mut app, Some(history_file_commands), None)
                .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        // The buffer should have the text "cd Desktop" with "Desktop" selected.
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("cd ", ctx);
            input.editor().update(ctx, |editor, ctx| {
                editor.insert_selected_text("Desktop/", ctx);
            });
        });

        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd Desktop/");
        });

        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![argument_suggestion("Documents/")],
                    (3, 4),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            )
        });

        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd Documents/");

            // The cursor should be at the end of the autocompleted text.
            let selection_range = input.editor().read(&app, |editor, ctx| {
                editor.start_byte_index_of_last_selection(ctx)
                    ..editor.end_byte_index_of_last_selection(ctx)
            });
            assert_eq!(selection_range, ByteOffset::from(13)..ByteOffset::from(13));
        });

        // Add more text after the inserted text and then reselect "Documents/". The editor will
        // ultimately have the text "cd Documents/foo/bar" with "Documents/" selected.
        input.update(&mut app, |input, ctx| {
            input.user_insert("foo/bar", ctx);
            input.editor().update(ctx, |editor, ctx| {
                editor
                    .select_ranges_by_byte_offset([ByteOffset::from(4)..ByteOffset::from(13)], ctx);
            });
        });

        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd Documents/foo/bar");
        });

        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![argument_suggestion("Desktop/")],
                    (3, 4),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });

        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd Desktop/foo/bar");

            // The cursor should be at the end of the autocompleted text (right after "Desktop/").
            let selection_range = input.editor().read(&app, |editor, ctx| {
                editor.start_byte_index_of_last_selection(ctx)
                    ..editor.end_byte_index_of_last_selection(ctx)
            });
            assert_eq!(selection_range, ByteOffset::from(11)..ByteOffset::from(11));
        });
    });
}

#[test]
fn test_tab_completion_longest_common_prefix() {
    // We need to check that we fill longest common prefix in two cases
    // Case 1: When user triggers a tab completion
    // Case 2: When user types to filter the completion results and then triggers tab completion again
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let suggestions = input.read(&app, |input, _ctx| input.input_suggestions.clone());

        // Case 1: When user triggers a tab completion, fill buffer with longest common prefix
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("open Cha", ctx);
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("Charlie1.txt"),
                        argument_suggestion("Charlie2.txt"),
                        argument_suggestion("Charlie3.txt"),
                        argument_suggestion("Charlie111_1.txt"),
                        argument_suggestion("Charlie111_2.txt"),
                        argument_suggestion("Charlie111_3.txt"),
                    ],
                    (5, 8),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "open Charlie");
        });

        // Case 2: When user types to filter the completion results and then triggers tab completion again,
        // fill buffer with longest common prefix of the filtered results
        input.update(&mut app, |input, ctx| {
            input.user_insert("11", ctx);
        });
        suggestions.update(&mut app, |suggestions, _| {
            suggestions.set_items(vec![
                Item::from_text("Charlie111_1.txt".to_string()),
                Item::from_text("Charlie111_2.txt".to_string()),
                Item::from_text("Charlie111_3.txt".to_string()),
            ]);
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "open Charlie111_");
        });
    });
}

#[test]
fn test_tab_completion_longest_common_prefix_with_fuzzy_suggestions_and_completions_open() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("open c", ctx);
        });
        input.update(&mut app, |input, ctx| {
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("charlie.txt"),
                        argument_suggestion("charlotte.txt"),
                        fuzzy_argument_suggestion("bobcha.txt", (3..=4).collect()),
                    ],
                    (5, 6),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        input.read(&app, |input, ctx| {
            // Tab completion menu should be open.
            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::CompletionSuggestions { .. }
            ))
        });
        input.update(&mut app, |input, ctx| {
            // Trigger tab completion when the completion menu is open.
            input.input_tab(ctx);
        });
        input.read(&app, |input, ctx| {
            // The common prefix between the two prefix matches should be inserted.
            assert_eq!(input.buffer_text(ctx), "open charl");
        });
    });
}

#[test]
fn test_tab_completion_hides_autosuggestion() {
    let _test = FeatureFlag::RemoveAutosuggestionDuringTabCompletions.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("open-file ", ctx);
            input.set_autosuggestion(
                "sesame",
                AutosuggestionType::Command {
                    was_intelligent_autosuggestion: false,
                },
                ctx,
            )
        });

        input.update(&mut app, |input, ctx| {
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![argument_suggestion("a.txt"), argument_suggestion("b.txt")],
                    (5, 5),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });

        input.read(&app, |input, ctx| {
            // Tab completion menu should be open.
            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::CompletionSuggestions { .. }
            ));

            // Autosuggestion should be closed.
            assert!(
                input
                    .editor
                    .as_ref(ctx)
                    .current_autosuggestion_text()
                    .is_none()
            );
        });
    });
}

#[test]
fn test_completions_while_typing_doesnt_hide_autosuggestion() {
    let _test = FeatureFlag::RemoveAutosuggestionDuringTabCompletions.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        InputSettings::handle(&app).update(&mut app, |input_settings, ctx| {
            let _ = input_settings
                .completions_open_while_typing
                .set_value(true, ctx);
        });

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("open-file ", ctx);
            input.set_autosuggestion(
                "sesame",
                AutosuggestionType::Command {
                    was_intelligent_autosuggestion: false,
                },
                ctx,
            )
        });

        // Autosuggestion should be active.
        input.read(&app, |input, ctx| {
            assert!(
                input
                    .editor
                    .as_ref(ctx)
                    .current_autosuggestion_text()
                    .is_some()
            );
        });

        input.update(&mut app, |input, ctx| {
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![argument_suggestion("a.txt"), argument_suggestion("b.txt")],
                    (5, 5),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });

        input.read(&app, |input, ctx| {
            // Tab completion menu should be open.
            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::CompletionSuggestions { .. }
            ));

            assert!(
                input
                    .editor
                    .as_ref(ctx)
                    .current_autosuggestion_text()
                    .is_some()
            );
        });
    });
}

#[test]
fn test_plan_slash_command_argument_with_slash_does_not_disable_slash_command_parsing() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.user_insert("/plan investigate app/src/main.rs", ctx);
        });

        input.read(&app, |input, ctx| {
            assert!(
                !input.slash_command_model.as_ref(ctx).is_disabled(),
                "slash command parsing should not be disabled when the argument contains '/'"
            );
        });
    });
}

#[test]
fn test_open_slash_command_triggers_completions_on_space() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let session_id: SessionId = 1.into();
        let session_info = SessionInfo::new_for_test().with_id(session_id);
        let terminal = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        simulate_directory_for_completion(session_id, &terminal, &mut app, "/tmp");

        input.update(&mut app, |input, ctx| {
            input.user_insert("/", ctx);
            input.user_insert("open-file ", ctx);
        });

        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "/open-file ");
            assert!(!matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::SlashCommands
            ));
            assert!(input.completions_abort_handle.is_some());
        });
    });
}

#[test]
fn test_open_slash_command_does_not_autofill_single_file_completion() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.editor.update(ctx, |editor, ctx| {
                editor.set_buffer_text("/open-file ", ctx)
            });
        });

        input.update(&mut app, |input, ctx| {
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![file_suggestion("test.md")],
                    (11, 11),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::SlashCommandAutoOpen,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "/open-file ");
        });

        input.update(&mut app, |input, ctx| {
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![file_suggestion("test.md")],
                    (11, 11),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "/open-file test.md ");
        });
    });
}

#[test]
fn test_open_slash_command_triggers_completions_when_selected() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let session_id: SessionId = 1.into();
        let session_info = SessionInfo::new_for_test().with_id(session_id);
        let terminal = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        simulate_directory_for_completion(session_id, &terminal, &mut app, "/tmp");

        input.update(&mut app, |input, ctx| {
            input.user_insert("/", ctx);
            input.handle_slash_commands_menu_event(
                &SlashCommandsEvent::SelectedStaticCommand {
                    id: COMMAND_REGISTRY
                        .get_command_id_with_name(commands::EDIT.name)
                        .copied()
                        .expect("open command should exist"),
                    cmd_or_ctrl_enter: false,
                },
                ctx,
            );
        });

        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "/open-file ");
            assert!(input.completions_abort_handle.is_some());
        });
    });
}

#[test]
fn test_open_slash_command_requires_path() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.editor.update(ctx, |editor, ctx| {
                editor.set_buffer_text("/open-file ", ctx)
            });
        });

        input.update(&mut app, |input, ctx| {
            input.input_enter(ctx);
        });
    });
}

#[test]
fn test_changelog_slash_command_clears_buffer_on_success() {
    App::test((), |mut app| async move {
        // `/changelog` is gated behind `FeatureFlag::Changelog` in two places, and only one
        // of them can be reached from a test. `all_commands` (`static_commands/commands.rs`)
        // only pushes `CHANGELOG` when the flag is on, and it feeds `COMMAND_REGISTRY`, a
        // process-global `LazyLock` that `input::init` dereferences during the first
        // `initialize_app` in the whole test binary. A thread-local `override_enabled` in
        // this test therefore cannot get `/changelog` into the registry, so driving this
        // through `input_enter` would depend on test order: `parse_slash_command` would not
        // match, `maybe_handle_enter_for_slash_command` would return false, and Enter would
        // run `/changelog` as a shell command (which deliberately does not clear the buffer
        // synchronously; `TerminalView` clears it on `BlockCompleted`).
        //
        // So execute the command directly, the way
        // `test_create_docker_sandbox_slash_command_executes_and_clears_buffer` does. The
        // override is still needed for the `SlashCommandKind::Changelog` arm of
        // `execute_slash_command`, which bails when the flag is off. The `input_enter` ->
        // slash command -> cleared buffer route stays covered by
        // `test_open_slash_command_clears_buffer_on_success`, whose `/open-file` is
        // registered unconditionally.
        let _changelog_flag = FeatureFlag::Changelog.override_enabled(true);
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.editor.update(ctx, |editor, ctx| {
                editor.set_buffer_text(commands::CHANGELOG.name, ctx)
            });
        });

        input.update(&mut app, |input, ctx| {
            let handled = input.execute_slash_command(
                &commands::CHANGELOG,
                None,
                SlashCommandTrigger::input(),
                /*is_queued_prompt*/ false,
                ctx,
            );
            assert!(handled, "/changelog should have been handled");
        });

        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "");
        });
    });
}
#[test]
fn test_open_slash_command_opens_files_palette_when_entered_from_slash_menu() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.user_insert("/", ctx);
            input.user_insert("open-file", ctx);
        });

        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "/open-file");
        });

        input.update(&mut app, |input, ctx| {
            input.input_enter(ctx);
        });
    });
}

#[cfg(feature = "local_fs")]
#[test]
fn test_open_slash_command_clears_buffer_on_success() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_file.txt");
        std::fs::File::create(&file_path).unwrap();

        let session_id: SessionId = 1.into();
        let session_info = SessionInfo::new_for_test().with_id(session_id);
        let terminal = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        simulate_directory_for_completion(
            session_id,
            &terminal,
            &mut app,
            temp_dir.to_string_lossy(),
        );

        input.update(&mut app, |input, ctx| {
            input.editor.update(ctx, |editor, ctx| {
                editor.set_buffer_text("/open-file test_file.txt", ctx)
            });
        });

        input.update(&mut app, |input, ctx| {
            input.input_enter(ctx);
        });

        input.read(&app, |input, ctx| {
            assert!(input.buffer_text(ctx).is_empty());
        });

        let _ = std::fs::remove_file(file_path);
    });
}

#[cfg(feature = "local_fs")]
#[test]
fn test_open_slash_command_expands_tilde() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let home_dir = dirs::home_dir().expect("home directory must exist");
        let file_path = home_dir.join("warp_tilde_test_file.txt");
        std::fs::File::create(&file_path).unwrap();

        let session_id: SessionId = 1.into();
        let session_info = SessionInfo::new_for_test().with_id(session_id);
        let terminal = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        // Simulate being in a directory that is NOT the home directory so we can
        // verify that ~ expansion takes priority over cwd joining.
        let temp_dir = std::env::temp_dir();
        simulate_directory_for_completion(
            session_id,
            &terminal,
            &mut app,
            temp_dir.to_string_lossy(),
        );

        input.update(&mut app, |input, ctx| {
            input.editor.update(ctx, |editor, ctx| {
                editor.set_buffer_text("/open-file ~/warp_tilde_test_file.txt", ctx)
            });
        });

        input.update(&mut app, |input, ctx| {
            input.input_enter(ctx);
        });

        // Buffer should be cleared on success, indicating the file was found.
        input.read(&app, |input, ctx| {
            assert!(input.buffer_text(ctx).is_empty());
        });

        let _ = std::fs::remove_file(file_path);
    });
}

#[test]
fn test_create_docker_sandbox_slash_command_executes_and_clears_buffer() {
    App::test((), |mut app| async move {
        let _docker_sandbox_flag = FeatureFlag::LocalDockerSandbox.override_enabled(true);
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.user_insert("draft text", ctx);
            let handled = input.execute_slash_command(
                &commands::CREATE_DOCKER_SANDBOX,
                None,
                SlashCommandTrigger::input(),
                /*is_queued_prompt*/ false,
                ctx,
            );
            assert!(handled);
        });

        input.read(&app, |input, ctx| {
            assert!(input.buffer_text(ctx).is_empty());
        });
    });
}

#[test]
fn test_tab_completion_single_prefix_suggestion_with_fuzzy_suggestions() {
    // If there is a single prefix suggestion with other fuzzy suggestions,
    // we should insert that prefix suggestion directly into the buffer
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("open cha", ctx);
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("cha.txt"),
                        fuzzy_argument_suggestion("bobcha.txt", (3..=5).collect()),
                    ],
                    (5, 8),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "open cha.txt ");
        });
    });
}

#[test]
fn test_tab_completion_only_fuzzy_suggestions() {
    // If there are only fuzzy suggestions, we don't insert a prefix even if there is a common prefix
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("open cha", ctx);
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        fuzzy_argument_suggestion("bobcha1.txt", (3..=5).collect()),
                        fuzzy_argument_suggestion("bobcha2.txt", (3..=5).collect()),
                    ],
                    (5, 8),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "open cha");
        });
    });
}

#[test]
fn test_tab_completion_prioritizes_longest_common_prefix_with_fuzzy_suggestions() {
    // If there are multiple prefix suggestions with any number of fuzzy suggestions,
    // the common prefix is inserted.
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let suggestions = input.read(&app, |input, _ctx| input.input_suggestions.clone());

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("open cha", ctx);
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("charlie1.txt"),
                        argument_suggestion("charlie2.txt"),
                        fuzzy_argument_suggestion("bobcha1.pdf", (3..=5).collect()),
                        fuzzy_argument_suggestion("bobcha11.pdf", (3..=5).collect()),
                    ],
                    (5, 8),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "open charlie");
        });

        // We also just check that we don't insert the common prefix when typing
        // to filter if there isn't a common prefix or the replacement
        // does not start the common prefix.
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("open cha1", ctx);
        });
        suggestions.update(&mut app, |suggestions, _| {
            suggestions.set_items(vec![
                Item::from_text("charlie1.txt".to_string()),
                Item::from_text("bobcha1.pdf".to_string()),
                Item::from_text("bobcha11.pdf".to_string()),
            ]);
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "open cha1");
        });

        input.update(&mut app, |input, ctx| {
            input.user_insert("p", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "open cha1p");
        });
        suggestions.update(&mut app, |suggestions, _| {
            suggestions.set_items(vec![
                Item::from_text("bobcha1.pdf".to_string()),
                Item::from_text("bobcha11.pdf".to_string()),
            ]);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "open cha1p");
        });
    });
}

#[test]
fn test_tab_completion_single_prefix_suggestion_after_fuzzy_suggestions() {
    // If there is a single prefix suggestion ordered after other fuzzy suggestions, we
    // insert that prefix suggestion directly into the buffer.
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("git a", ctx);
        });

        input.update(&mut app, |input, ctx| {
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        fuzzy_argument_suggestion("dab", vec![4]),
                        argument_suggestion("add"),
                    ],
                    (4, 5),
                    MatchStrategy::Fuzzy,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            )
        });

        input.update(&mut app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "git add ");
        });
    });
}

#[test]
fn test_tab_completion_case_sensitive_single_suggestion() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("open ab", ctx);
        });

        input.update(&mut app, |input, ctx| {
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("abc.txt"),
                        case_insensitive_argument_suggestion("Abcd.txt"),
                    ],
                    (5, 6),
                    MatchStrategy::Fuzzy,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            )
        });

        input.update(&mut app, |input, ctx| {
            // There is only 1 case-sensitive prefix suggestion, so we insert it
            assert_eq!(input.buffer_text(ctx), "open abc.txt ");
        });

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("open ab", ctx);
        });

        input.update(&mut app, |input, ctx| {
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        case_insensitive_argument_suggestion("Abc.txt"),
                        fuzzy_argument_suggestion("bobabc.txt", (3..=4).collect()),
                    ],
                    (5, 6),
                    MatchStrategy::Fuzzy,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            )
        });

        input.update(&mut app, |input, ctx| {
            // There are no case-sensitive prefixes, but 1 case-insensitive prefix,
            // suggestion, so we insert it.
            assert_eq!(input.buffer_text(ctx), "open Abc.txt ");
        });
    });
}

#[test]
fn test_tab_completion_case_sensitivity_common_prefix() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("open ab", ctx);
        });

        input.update(&mut app, |input, ctx| {
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("abcdef.txt"),
                        argument_suggestion("abcdag.txt"),
                        case_insensitive_argument_suggestion("Abcd.txt"),
                    ],
                    (5, 6),
                    MatchStrategy::Fuzzy,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            )
        });

        input.update(&mut app, |input, ctx| {
            // Insert the common prefix for the case-sensitive suggestions.
            assert_eq!(input.buffer_text(ctx), "open abcd");
        });
    });
}

#[test]
fn test_tab_completion_case_insensitive_exact_match() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("abc", ctx);
        });

        input.update(&mut app, |input, ctx| {
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("abcdef"),
                        case_insensitive_exact_argument_suggestion("Abc"),
                    ],
                    (0, 3),
                    MatchStrategy::Fuzzy,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            )
        });

        input.update(&mut app, |input, ctx| {
            // Single case-sensitive prefix suggestions are inserted even if there's
            // a case-insensitive exact match.
            assert_eq!(input.buffer_text(ctx), "abcdef ");
        });

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("abc", ctx);
        });

        input.update(&mut app, |input, ctx| {
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("abcdef"),
                        argument_suggestion("abcdeg"),
                        case_insensitive_exact_argument_suggestion("Abc"),
                    ],
                    (0, 3),
                    MatchStrategy::Fuzzy,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            )
        });

        input.update(&mut app, |input, ctx| {
            // Case-sensitive common prefixes are inserted even if there's a
            // case-insensitive exact match.
            assert_eq!(input.buffer_text(ctx), "abcde");
        });
    });
}

#[test]
fn test_tab_completion_longest_common_prefix_with_fuzzy_suggestions() {
    // We want to test the following behaviour:
    // 1. If there is a single prefix suggestion with other fuzzy suggestions,
    //    we should insert that prefix suggestion directly into the buffer
    // 2. If there are only fuzzy suggestions, we don't insert a prefix even if there is a common prefix
    // 3. If there is a single prefix suggestion ordered after other fuzzy suggestions, we
    //     insert that prefix suggestion directly into the buffer.
    // We also check that this behaviour works when typing to filter.
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let suggestions = input.read(&app, |input, _ctx| input.input_suggestions.clone());

        // Case 1. If there is a single prefix suggestion with other fuzzy suggestions, we should insert that prefix suggestion
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("open cha", ctx);
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("cha.txt"),
                        fuzzy_argument_suggestion("bobcha.txt", (3..=5).collect()),
                    ],
                    (5, 8),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "open cha.txt ");
        });

        // Case 2. If there are only fuzzy suggestions, we don't insert a prefix even if there is a common prefix
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("open cha", ctx);
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        fuzzy_argument_suggestion("bobcha1.txt", (3..=5).collect()),
                        fuzzy_argument_suggestion("bobcha2.txt", (3..=5).collect()),
                    ],
                    (5, 8),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "open cha");
        });

        // We also just check that we don't insert the common prefix when typing
        // to filter if there isn't a common prefix or the replacement
        // does not start the common prefix.
        input.update(&mut app, |input, ctx| {
            input.user_insert("1", ctx);
        });
        suggestions.update(&mut app, |suggestions, _| {
            suggestions.set_items(vec![
                Item::from_text("charlie1.txt".to_string()),
                Item::from_text("bobcha1.pdf".to_string()),
                Item::from_text("bobcha11.pdf".to_string()),
            ]);
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "open cha1");
        });

        input.update(&mut app, |input, ctx| {
            input.user_insert("p", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "open cha1p");
        });
        suggestions.update(&mut app, |suggestions, _| {
            suggestions.set_items(vec![
                Item::from_text("bobcha1.pdf".to_string()),
                Item::from_text("bobcha11.pdf".to_string()),
            ]);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "open cha1p");
        });

        // Case 3: Ensure that the prefix suggestion is inserted, even if it's not the first
        // ordered suggestion.
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("git a", ctx);
        });

        input.update(&mut app, |input, ctx| {
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        fuzzy_argument_suggestion("dab", vec![4]),
                        argument_suggestion("add"),
                    ],
                    (4, 5),
                    MatchStrategy::Fuzzy,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            )
        });

        input.update(&mut app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "git add ");
        });
    });
}

#[test]
fn test_tab_completion_common_prefix_shorter() {
    // We need to check the same two cases as the 'longest_common_prefix' test, however we want
    // to verify that if the longest common prefix is _shorter_ than what the user typed, we
    // don't insert it
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let suggestions = input.read(&app, |input, _| input.input_suggestions.clone());

        // Case 1: When a user triggers a tab completion, ensure longest common prefix is
        // longer than the text
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("cd foo/b", ctx);
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("foo/Bar"),
                        argument_suggestion("foo/bazz"),
                    ],
                    (3, 8),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd foo/b");
        });

        // Case 2: When user types to filter the completion results and then triggers tab
        // completion again, we still want to ensure the longest common prefix is longer
        // than the text
        input.update(&mut app, |input, ctx| {
            input.close_input_suggestions(/*should_focus_input=*/ true, ctx);
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("cd f", ctx);
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("far"),
                        argument_suggestion("foo/Bar"),
                        argument_suggestion("foo/bazz"),
                    ],
                    (3, 4),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
            input.user_insert("oo/b", ctx);
        });
        suggestions.update(&mut app, |suggestions, _| {
            suggestions.set_items(vec![
                Item::from_text("foo/Bar".into()),
                Item::from_text("foo/bazz".into()),
            ]);
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd foo/b");
        });
    });
}

#[test]
fn test_cursor_movement() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let history_file_commands = vec![
            "cd Documents/zed".to_string(),
            "curl https://app.warp.dev".to_string(),
            "cargo check\ncargo run".to_string(),
        ];
        let terminal =
            add_window_with_bootstrapped_terminal(&mut app, Some(history_file_commands), None)
                .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let editor = input.read(&app, |input, _| input.editor.clone());
        // Test cursor movement
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("c", ctx);
            input.user_insert("d", ctx);
            input.user_insert(" ", ctx);
            input.user_insert("D", ctx);
        });

        // XXX Note that it's necessary to put `input_tab` in a separate call.
        // Otherwise, there's a race where we crash because editor:cursor is not set.
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd D");
        });

        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("Downloads"),
                        argument_suggestion("Documents"),
                    ],
                    (3, 4),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        let expected_completion = InputSuggestionsMode::CompletionSuggestions {
            replacement_start: 3,
            buffer_text_original: "cd D".to_string(),
            completion_results: SuggestionResults {
                suggestions: vec![
                    argument_suggestion("Downloads"),
                    argument_suggestion("Documents"),
                ],
                replacement_span: Span::new(3, 4),
                match_strategy: MatchStrategy::CaseInsensitive,
            },
            trigger: CompletionsTrigger::Keybinding,
            menu_position: TabCompletionsMenuPosition::AtLastCursor,
        };
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd Do");
            assert_eq!(
                *input.suggestions_mode_model().as_ref(ctx).mode(),
                expected_completion
            );
        });
        // move back 1 character, and we're still showing the completion, except ignoring the
        // characters _after_ the cursor
        editor.update(&mut app, |editor, ctx| {
            editor.move_left(/* stop at line start */ false, ctx)
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd Do");
            assert_eq!(
                *input.suggestions_mode_model().as_ref(ctx).mode(),
                expected_completion
            );
        });
        editor.read(&app, |editor, ctx| {
            assert!(editor.is_single_cursor_only(ctx));
            let column = editor.start_byte_index_of_last_selection(ctx).as_usize();
            assert_eq!(column, 4);
        });

        // Put the cursor back at the end
        editor.update(&mut app, |editor, ctx| {
            editor.move_right(/* stop at line end */ false, ctx);
        });

        editor.read(&app, |editor, ctx| {
            assert!(editor.is_single_cursor_only(ctx));
            let column = editor.start_byte_index_of_last_selection(ctx).as_usize();
            assert_eq!(column, 5);
        });
    });
}

#[cfg_attr(windows, ignore = "TODO(CORE-3626)")]
#[test]
fn test_newline_insertion() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let editor = input.read(&app, |input, _| input.editor().clone());

        // Fill in the buffer with `ls \`
        editor.update(&mut app, |editor, ctx| {
            editor.user_insert(r"ls \", ctx);
        });

        // There should only be one line.
        editor.read(&app, |editor, ctx| {
            assert_eq!(editor.max_point(ctx).row(), 0);
        });

        // Move cursor to the end of the first line
        editor.update(&mut app, |input, ctx| {
            let line_0_end = DisplayPoint::new(0, input.line_len(0, ctx).unwrap());
            input
                .select_ranges(Some(line_0_end..line_0_end), ctx)
                .unwrap();
        });

        // Handle a return
        input.update(&mut app, |input, ctx| {
            input.input_enter(ctx);
        });

        // We should have inserted a newline
        editor.read(&app, |editor, ctx| {
            assert_eq!(editor.max_point(ctx).row(), 1);
        });
    })
}

#[test]
fn test_should_not_insert_newline_on_enter_in_empty_buffer() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;

        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.read(&app, |input, ctx| {
            assert!(input.buffer_text(ctx).is_empty());
            assert!(!input.should_insert_newline_on_enter(ctx));
        });
    })
}

#[cfg_attr(windows, ignore = "TODO(CORE-3626)")]
#[test]
fn test_should_insert_newline_on_enter() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let base_text = r"
            1 slash \
            2 slashes \\
            3 slashes \\\
            4 slashes \\\\
            no slashes
        "
        .unindent();

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;

        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        input.update(&mut app, |input, ctx| {
            input.replace_buffer_content(base_text.as_str(), ctx);
            input.editor.update(ctx, |editor, ctx| {
                editor
                    .select_ranges(vec![DisplayPoint::new(0, 0)..DisplayPoint::new(0, 0)], ctx)
                    .unwrap();
            })
        });

        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), base_text);
            assert!(input.editor.as_ref(ctx).single_cursor_on_first_line(ctx));
        });

        input.update(&mut app, |input, ctx| {
            // Move cursor to end of first line.
            input.editor.update(ctx, |editor, ctx| {
                editor.move_to_line_end(ctx);
            });
            assert!(input.should_insert_newline_on_enter(ctx));

            // Move cursor to end of second line.
            input.editor.update(ctx, |editor, ctx| {
                editor.move_down(ctx);
                editor.move_to_line_end(ctx);
            });
            assert!(!input.should_insert_newline_on_enter(ctx));

            // Move cursor to end of third line.
            input.editor.update(ctx, |editor, ctx| {
                editor.move_down(ctx);
                editor.move_to_line_end(ctx);
            });
            assert!(input.should_insert_newline_on_enter(ctx));

            // Move cursor to end of fourth line.
            input.editor.update(ctx, |editor, ctx| {
                editor.move_down(ctx);
                editor.move_to_line_end(ctx);
            });
            assert!(!input.should_insert_newline_on_enter(ctx));

            // Move cursor to end of fifth line.
            input.editor.update(ctx, |editor, ctx| {
                editor.move_down(ctx);
                editor.move_to_line_end(ctx);
            });
            assert!(!input.should_insert_newline_on_enter(ctx));
        });
    })
}

#[test]
fn test_powershell_should_insert_newline_on_enter() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let base_text = r"
            1 slash \
            1 backtick with space `
            1 backtick no space f`
            no backtick
            2 backticks ``
            3 backticks ```
        "
        .unindent();

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;

        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        input.update(&mut app, |input, ctx| {
            input.replace_buffer_content(base_text.as_str(), ctx);
            input.editor.update(ctx, |editor, ctx| {
                editor
                    .select_ranges(vec![DisplayPoint::new(0, 0)..DisplayPoint::new(0, 0)], ctx)
                    .unwrap();
                editor.set_shell_family(ShellFamily::PowerShell);
            })
        });

        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), base_text);
            assert!(input.editor.as_ref(ctx).single_cursor_on_first_line(ctx));
        });

        input.update(&mut app, |input, ctx| {
            // Move cursor to end of first line.
            input.editor.update(ctx, |editor, ctx| {
                editor.move_to_line_end(ctx);
            });
            assert!(!input.should_insert_newline_on_enter(ctx));

            // Move cursor to end of second line.
            input.editor.update(ctx, |editor, ctx| {
                editor.move_down(ctx);
                editor.move_to_line_end(ctx);
            });
            assert!(input.should_insert_newline_on_enter(ctx));

            input.editor.update(ctx, |editor, ctx| {
                editor.move_down(ctx);
                editor.move_to_line_end(ctx);
            });
            assert!(!input.should_insert_newline_on_enter(ctx));

            input.editor.update(ctx, |editor, ctx| {
                editor.move_down(ctx);
                editor.move_to_line_end(ctx);
            });
            assert!(!input.should_insert_newline_on_enter(ctx));

            input.editor.update(ctx, |editor, ctx| {
                editor.move_down(ctx);
                editor.move_to_line_end(ctx);
            });
            assert!(!input.should_insert_newline_on_enter(ctx));

            input.editor.update(ctx, |editor, ctx| {
                editor.move_down(ctx);
                editor.move_to_line_end(ctx);
            });
            assert!(!input.should_insert_newline_on_enter(ctx));
        });
    })
}

#[test]
fn test_workflow_selected() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        input.update(&mut app, |input, ctx| {
            input.user_insert("hello", ctx);
        });

        let workflow = Workflow::new(
            "test",
            "{{p1}} {{parameter_2}} {{p3}} foo {{p1}} {{parameter_2}}",
        )
        .with_arguments(vec![
            Argument::new("p1", ArgumentType::Text),
            Argument::new("parameter_2", ArgumentType::Text),
            Argument::new("p3", ArgumentType::Text),
        ]);

        input.update(&mut app, |input, ctx| {
            input.show_workflows_info_box_on_workflow_selection(
                WorkflowType::Local(workflow),
                WorkflowSource::Global,
                WorkflowSelectionSource::Undefined,
                None,
                ctx,
            );
        });

        input.read(&app, |input, ctx| {
            assert_eq!(
                input.buffer_text(ctx),
                "p1 parameter_2 p3 foo p1 parameter_2"
            );
        });
    });
}

#[test]
fn test_workflow_selected_with_default_value() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        let workflow = Workflow::new("test", "{{p1}}/{{parameter_2}}").with_arguments(vec![
            Argument {
                name: "p1".into(),
                description: None,
                default_value: Some("default_parameter_1".into()),
                arg_type: Default::default(),
            },
            Argument {
                name: "parameter_2".into(),
                description: None,
                default_value: Some("default_parameter_2".into()),
                arg_type: Default::default(),
            },
        ]);

        input.update(&mut app, |input, ctx| {
            input.show_workflows_info_box_on_workflow_selection(
                WorkflowType::Local(workflow),
                WorkflowSource::Global,
                WorkflowSelectionSource::Undefined,
                None,
                ctx,
            );
        });

        input.read(&app, |input, ctx| {
            assert_eq!(
                input.buffer_text(ctx),
                "default_parameter_1/default_parameter_2"
            );
        });
    });
}

#[test]
fn test_multiple_workflows_selected() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        let workflow = Workflow::new("test", "p1 {{foo}} bar")
            .with_arguments(vec![Argument::new("foo", ArgumentType::Text)]);

        input.update(&mut app, |input, ctx| {
            input.show_workflows_info_box_on_workflow_selection(
                WorkflowType::Local(workflow.clone()),
                WorkflowSource::Global,
                WorkflowSelectionSource::Undefined,
                None,
                ctx,
            );
        });

        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "p1 foo bar");
        });

        // "foo" should be the only range highlighted.
        input.update(&mut app, |input, ctx| {
            let text_style_runs = input.editor.read(ctx, |editor, ctx| {
                editor
                    .text_style_runs(ctx)
                    .filter_map(|text_run| {
                        text_run
                            .text_style()
                            .background_color
                            .map(|_| text_run.text().to_owned())
                    })
                    .collect::<Vec<_>>()
            });

            assert_eq!(text_style_runs, ["foo"]);
        });

        // Input the workflow again.
        input.update(&mut app, |input, ctx| {
            input.show_workflows_info_box_on_workflow_selection(
                WorkflowType::Local(workflow),
                WorkflowSource::Global,
                WorkflowSelectionSource::Undefined,
                None,
                ctx,
            );
        });

        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "p1 foo bar");
        });

        // "foo" should be the only range highlighted.
        input.update(&mut app, |input, ctx| {
            let text_style_runs = input.editor.read(ctx, |editor, ctx| {
                editor
                    .text_style_runs(ctx)
                    .filter_map(|text_run| {
                        text_run
                            .text_style()
                            .background_color
                            .map(|_| text_run.text().to_owned())
                    })
                    .collect::<Vec<_>>()
            });

            assert_eq!(text_style_runs, ["foo"]);
        });
    });
}

#[test]
fn test_workflow_argument_tab_with_syntax_highlighting() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;

        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        let workflow = Workflow::new("test", "yarn {{cwd}} {{flags}}").with_arguments(vec![
            Argument {
                name: "cwd".into(),
                description: None,
                default_value: Some("--cwd ./".into()),
                arg_type: Default::default(),
            },
            Argument::new("flags", ArgumentType::Text),
        ]);

        input.update(&mut app, |input, ctx| {
            input.show_workflows_info_box_on_workflow_selection(
                WorkflowType::Local(workflow.clone()),
                WorkflowSource::Global,
                WorkflowSelectionSource::Undefined,
                None,
                ctx,
            );

            // Simulates syntax highlighting highlighting a portion of an argument
            input.editor.update(ctx, |editor, ctx| {
                let theme = Appearance::as_ref(ctx).theme();
                let terminal_colors_normal = theme.terminal_colors().normal.to_owned();
                editor.update_buffer_styles(
                    vec![ByteOffset::from(5)..ByteOffset::from(10)],
                    TextStyleOperation::default().set_syntax_color(
                        AnsiColorIdentifier::Yellow
                            .to_ansi_color(&terminal_colors_normal)
                            .into(),
                    ),
                    ctx,
                )
            })
        });

        // Even though there are 2 args, there will be 3 runs
        input.read(&app, |input, ctx| {
            // Buffer text should equal our command w/ defaults inserted
            assert_eq!(input.buffer_text(ctx), "yarn --cwd ./ flags");

            let selected_text = input
                .editor
                .read(ctx, |editor, ctx| editor.selected_text(ctx));

            // Currently selected text should be the text for the first arg
            assert_eq!(selected_text, "--cwd ./");

            let text_style_runs = input.editor.read(ctx, |editor, ctx| {
                editor
                    .text_style_runs(ctx)
                    .filter_map(|text_run| {
                        text_run
                            .text_style()
                            .background_color
                            .map(|_| text_run.text().to_owned())
                    })
                    .collect::<Vec<_>>()
            });

            // Even though we have only 2 args, there will be 3 runs b/c of syntax highlighting
            assert_eq!(text_style_runs, ["--cwd", " ./", "flags"]);
        });

        input.update(&mut app, |input, ctx| {
            input.input_shift_tab(ctx);
        });

        input.read(&app, |input, ctx| {
            let selected_text = input
                .editor
                .read(ctx, |editor, ctx| editor.selected_text(ctx));

            // Tab moves over to next argument
            assert_eq!(selected_text, "flags");
        })
    })
}

#[test]
fn test_workflow_view_does_not_panic() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;

        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        let workflows = vec![
            Workflow::new("Test Workflow", "echo \"Hello World\""),
            Workflow::new("Test Workflow with Description", "echo \"Hello World\"")
                .with_description("This is a test workflow that prints Hello World!".into()),
            Workflow::new("Test Workflow with Args", "echo \"Hello {{person}}\"").with_arguments(
                vec![
                    Argument::new("person", ArgumentType::Text)
                        .with_description("The person you want to say hello to".to_string()),
                ],
            ),
            Workflow::new("test", "echo \"Hello {{person}}\"")
                .with_description("This is a test workflow that prints Hello {{person}}!".into())
                .with_arguments(vec![
                    Argument::new("person", ArgumentType::Text)
                        .with_description("The person you want to say hello to".to_string()),
                ]),
        ];

        for workflow in workflows {
            let command = workflow.content().to_string();
            input.update(&mut app, |input, ctx| {
                input.show_workflows_info_box_on_workflow_selection(
                    WorkflowType::Local(workflow),
                    WorkflowSource::Global,
                    WorkflowSelectionSource::Undefined,
                    None,
                    ctx,
                );
            });

            input.read(&app, |input, ctx| {
                // Buffer text should equal our command w/ defaults inserted
                assert_eq!(
                    input.buffer_text(ctx),
                    command.replace("{{", "").replace("}}", "")
                );
            });
        }
    })
}

#[test]
fn test_system_insert() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        input.update(&mut app, |input, ctx| {
            input.system_insert("hello world", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(
                input.buffer_text(ctx),
                "hello world",
                "Should have inserted 'hello world'"
            );
        });
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
        });
        input.read(&app, |input, ctx| {
            assert!(input.buffer_text(ctx).is_empty(), "Input should be empty");
        });
        input.update(&mut app, |input, ctx| {
            input.system_insert("hello\nworld", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(
                input.buffer_text(ctx),
                "hello\nworld",
                "Should have inserted 'hello\nworld'"
            );
        });
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
        });
        input.read(&app, |input, ctx| {
            assert!(input.buffer_text(ctx).is_empty(), "Input should be empty");
        });
        input.update(&mut app, |input, ctx| {
            input.system_insert("héłló worlḏ", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(
                input.buffer_text(ctx),
                "héłló worlḏ",
                "Should have inserted 'héłló worlḏ'"
            );
        });
    });
}

#[test]
fn test_is_cursor_in_valid_position_for_completions_while_typing() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;
        let (input, editor) = terminal.read(&app, |terminal, ctx| {
            let input = terminal.input().clone();
            let editor = input.as_ref(ctx).editor().clone();
            (input, editor)
        });
        input.update(&mut app, |input, ctx| {
            input.set_active_block_metadata(
                BlockMetadata::new(Some(SessionId::from(0)), Some("~".into())),
                false,
                ctx,
            )
        });

        // If cursor is at end of line, show completions menu
        input.update(&mut app, |input, ctx| {
            input.user_insert("gi", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Buffer now looks like "gi|"
        });
        input.update(&mut app, |input, ctx| {
            assert!(input.is_cursor_in_valid_position_for_completions_while_typing(ctx));
        });

        // If cursor is not at end of line, don't show completions menu
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_start(ctx);
            // Buffer now looks like "|gi"
        });
        input.update(&mut app, |input, ctx| {
            assert!(!input.is_cursor_in_valid_position_for_completions_while_typing(ctx));
        });

        // Even if cursor is at end of line when there's multiple lines, don't show
        // completions unless its at the end of the last line.
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Buffer now looks like " gi|"
        });

        input.update(&mut app, |input, ctx| {
            input.user_insert("\ngi", ctx);
            // Buffer currently looks like "gi\ngi|"
            assert_eq!(input.buffer_text(ctx), "gi\ngi");
            assert!(input.is_cursor_in_valid_position_for_completions_while_typing(ctx));
        });

        editor.update(&mut app, |editor, ctx| {
            // Close the tab completion menu if open
            editor.escape(ctx);
            editor.move_up(ctx);
            // Buffer now looks like "gi|\ngi"
        });

        input.update(&mut app, |input, ctx| {
            assert!(!input.is_cursor_in_valid_position_for_completions_while_typing(ctx));
        });

        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Buffer now looks like "gi\ngi|"
        });
        input.update(&mut app, |input, ctx| {
            assert!(input.is_cursor_in_valid_position_for_completions_while_typing(ctx));
        });
    });
}

#[test]
fn test_last_word_insertions() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        // last word insertion looks for preceding whitespace character
        let history_file_commands = vec![
            "https://app.warp.dev".to_string(),
            "cargo check\ncargo run --features".to_string(),
        ];
        let terminal =
            add_window_with_bootstrapped_terminal(&mut app, Some(history_file_commands), None)
                .await;

        let (input, editor) = terminal.read(&app, |terminal, ctx| {
            let input = terminal.input().clone();
            let editor = input.as_ref(ctx).editor().clone();
            (input, editor)
        });

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("git test", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "git test");
        });

        // Insert while selecting the word `test`
        editor.update(&mut app, |editor, ctx| {
            editor.select_word(&DisplayPoint::new(0, 4), ctx);
        });
        input.update(&mut app, |input, ctx| {
            input.insert_last_word_previous_command(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "git --features");
        });

        // Next insert replaces inserted word (not all of current text), with word from second last history command
        input.update(&mut app, |input, ctx| {
            input.insert_last_word_previous_command(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "git https://app.warp.dev");
        });

        // Insert is temporary, undo goes back to initial state before first insertion
        // After undo, `test` is currently selected
        editor.update(&mut app, |editor, ctx| {
            editor.undo(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "git test");
        });

        // After system edit action (undo), subsequent inserts will insert last word of most recent command
        // After insert, `--features` is currently selected
        input.update(&mut app, |input, ctx| {
            input.insert_last_word_previous_command(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "git --features");
        });

        // After user edit action (input), subsequent inserts will insert last word of most recent command
        editor.update(&mut app, |editor, ctx| {
            editor.user_insert("f", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "git f");
        });
        // Cursor after `f`
        input.update(&mut app, |input, ctx| {
            input.insert_last_word_previous_command(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "git f--features");
        });

        // After non-edit action (move left), subsequent inserts will insert last word of most recent command
        editor.update(&mut app, |editor, ctx| {
            editor.move_left(/* stop at line start */ false, ctx);
            editor.move_left(/* stop at line start */ false, ctx);
        });
        input.update(&mut app, |input, ctx| {
            input.insert_last_word_previous_command(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "git --featuresf--features");
        });
    });
}

#[test]
fn test_last_word_insertions_multiline() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let history_file_commands = vec![
            "git status".to_string(),
            "cargo check\ncargo run".to_string(),
        ];
        let terminal =
            add_window_with_bootstrapped_terminal(&mut app, Some(history_file_commands), None)
                .await;

        let (input, editor) = terminal.read(&app, |terminal, ctx| {
            let input = terminal.input().clone();
            let editor = input.as_ref(ctx).editor().clone();
            (input, editor)
        });

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("git test\ngit two\ngit three", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "git test\ngit two\ngit three");
        });

        editor.update(&mut app, |editor, ctx| {
            editor
                .select_ranges(
                    vec![
                        DisplayPoint::new(0, 4)..DisplayPoint::new(0, 6),
                        DisplayPoint::new(1, 4)..DisplayPoint::new(1, 6),
                        DisplayPoint::new(2, 4)..DisplayPoint::new(2, 6),
                    ],
                    ctx,
                )
                .unwrap();
        });
        input.update(&mut app, |input, ctx| {
            input.insert_last_word_previous_command(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "git runst\ngit runo\ngit runree");
        });

        // Insert again.
        input.update(&mut app, |input, ctx| {
            input.insert_last_word_previous_command(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(
                input.buffer_text(ctx),
                "git statusst\ngit statuso\ngit statusree"
            );
        });

        // On selection change, reset to inserting latest in history.
        editor.update(&mut app, |editor, ctx| {
            editor
                .select_ranges(vec![DisplayPoint::new(0, 5)..DisplayPoint::new(0, 6)], ctx)
                .unwrap();
        });
        editor.update(&mut app, |editor, ctx| {
            editor.delete(ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor
                .select_ranges(
                    vec![
                        DisplayPoint::new(0, 4)..DisplayPoint::new(0, 6),
                        DisplayPoint::new(1, 4)..DisplayPoint::new(1, 6),
                        DisplayPoint::new(2, 4)..DisplayPoint::new(2, 6),
                    ],
                    ctx,
                )
                .unwrap();
        });

        input.update(&mut app, |input, ctx| {
            input.insert_last_word_previous_command(ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(
                input.buffer_text(ctx),
                "git runtusst\ngit runatuso\ngit runatusree"
            );
        });
    });
}

#[test]
fn test_alias_expansion() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let aliases = HashMap::from_iter([("gco".into(), "git checkout".into())]);
        let session_info = SessionInfo::new_for_test().with_aliases(aliases);

        set_alias_expansion_setting(true, &mut app);
        let terminal = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;
        let (input, editor) = terminal.read(&app, |terminal, ctx| {
            let input = terminal.input().clone();
            let editor = input.as_ref(ctx).editor().clone();
            (input, editor)
        });
        input.update(&mut app, |input, ctx| {
            input.set_active_block_metadata(
                BlockMetadata::new(Some(SessionId::from(0)), Some("~".into())),
                false,
                ctx,
            )
        });

        // Commands are expanded when cursor is at end of line
        input.update(&mut app, |input, ctx| {
            input.user_insert("gco ", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Cursor is now at "gco |"
        });
        input.update(&mut app, |input, ctx| {
            input.run_expansion_on_space(ctx);
            assert_eq!(input.buffer_text(ctx), "git checkout ");
        });

        // Commands are expanded when cursor is in middle of the line
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("gco test", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            use crate::editor::EditorAction;
            editor.move_to_buffer_end(ctx);
            editor.handle_action(&EditorAction::MoveBackwardOneWord, ctx);
            // Cursor is now at "gco |test"
        });
        input.update(&mut app, |input, ctx| {
            input.run_expansion_on_space(ctx);
            assert_eq!(input.buffer_text(ctx), "git checkout test");
        });
    });
}

#[test]
fn test_alias_expansion_multiple_commands_in_input() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let aliases = HashMap::from_iter([("gco".into(), "git checkout".into())]);
        let session_info = SessionInfo::new_for_test().with_aliases(aliases);

        set_alias_expansion_setting(true, &mut app);
        let terminal = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;
        let (input, editor) = terminal.read(&app, |terminal, ctx| {
            let input = terminal.input().clone();
            let editor = input.as_ref(ctx).editor().clone();
            (input, editor)
        });
        input.update(&mut app, |input, ctx| {
            input.set_active_block_metadata(
                BlockMetadata::new(Some(SessionId::from(0)), Some("~".into())),
                false,
                ctx,
            )
        });

        // Multilined commands are expanded
        input.update(&mut app, |input, ctx| {
            input.user_insert("test \ngco ", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Cursor is now at "test \ngco |"
        });
        input.update(&mut app, |input, ctx| {
            input.run_expansion_on_space(ctx);
            assert_eq!(input.buffer_text(ctx), "test \ngit checkout ");
        });

        // Mulitlined commands with multiple cursors are not expanded
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("gco \ngco ", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            use crate::editor::EditorAction;
            editor.move_to_buffer_end(ctx);
            editor.handle_action(&EditorAction::AddCursorAbove, ctx);
            // Cursor is now at "gco |\ngco |"
        });
        input.update(&mut app, |input, ctx| {
            input.run_expansion_on_space(ctx);
            assert_eq!(input.buffer_text(ctx), "gco \ngco ");
        });

        // Chained commands are expanded
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("vim && gco ", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Cursor is now at "vim && gco |"
        });
        input.update(&mut app, |input, ctx| {
            input.run_expansion_on_space(ctx);
            assert_eq!(input.buffer_text(ctx), "vim && git checkout ");
        });

        // Nested commands are expanded
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("cd $(gco ", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Cursor is now at "cd $(gco |"
        });
        input.update(&mut app, |input, ctx| {
            input.run_expansion_on_space(ctx);
            assert_eq!(input.buffer_text(ctx), "cd $(git checkout ");
        });
    });
}

#[test]
fn test_alias_expansion_when_invalid_expansion() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let aliases = HashMap::from_iter([("gco".into(), "git checkout".into())]);
        let session_info = SessionInfo::new_for_test().with_aliases(aliases);

        set_alias_expansion_setting(true, &mut app);
        let terminal = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;
        let (input, editor) = terminal.read(&app, |terminal, ctx| {
            let input = terminal.input().clone();
            let editor = input.as_ref(ctx).editor().clone();
            (input, editor)
        });
        input.update(&mut app, |input, ctx| {
            input.set_active_block_metadata(
                BlockMetadata::new(Some(SessionId::from(0)), Some("~".into())),
                false,
                ctx,
            )
        });

        // No expansion if the token is an argument
        input.update(&mut app, |input, ctx| {
            input.user_insert("test gco ", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Cursor is now at "test gco |"
        });
        input.update(&mut app, |input, ctx| {
            input.run_expansion_on_space(ctx);
            assert_eq!(input.buffer_text(ctx), "test gco ");
        });

        // No expansion if the token is not an alias
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("test ", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Cursor is now at "test |"
        });
        input.update(&mut app, |input, ctx| {
            input.run_expansion_on_space(ctx);
            assert_eq!(input.buffer_text(ctx), "test ");
        });
    });
}

#[test]
fn test_alias_expansion_when_alias_includes_itself() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let aliases =
            HashMap::from_iter([("g".into(), "git".into()), ("ls".into(), "ls -G".into())]);
        let session_info = SessionInfo::new_for_test().with_aliases(aliases);

        set_alias_expansion_setting(true, &mut app);
        let terminal = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;
        let (input, editor) = terminal.read(&app, |terminal, ctx| {
            let input = terminal.input().clone();
            let editor = input.as_ref(ctx).editor().clone();
            (input, editor)
        });
        input.update(&mut app, |input, ctx| {
            input.set_active_block_metadata(
                BlockMetadata::new(Some(SessionId::from(0)), Some("~".into())),
                false,
                ctx,
            )
        });

        // An alias that includes itself is not expanded
        input.update(&mut app, |input, ctx| {
            input.user_insert("ls ", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Cursor is now at "ls |"
        });
        input.update(&mut app, |input, ctx| {
            input.run_expansion_on_space(ctx);
            assert_eq!(input.buffer_text(ctx), "ls ");
        });

        // Aliases that are only a substring of the alias value are still expanded
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("g ", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Cursor is now at "g |"
        });
        input.update(&mut app, |input, ctx| {
            input.run_expansion_on_space(ctx);
            assert_eq!(input.buffer_text(ctx), "git ");
        });
    });
}

#[test]
fn test_alias_expansion_with_abbreviations() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let abbreviations = HashMap::from_iter([("g".into(), "git log".into())]);
        let aliases = HashMap::from_iter([("g".into(), "git".into())]);
        let session_info = SessionInfo::new_for_test()
            .with_aliases(aliases)
            .with_abbreviations(abbreviations);

        set_alias_expansion_setting(true, &mut app);
        let terminal = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;

        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let editor = input.read(&app, |input, _| input.editor().clone());

        input.update(&mut app, |input, ctx| {
            input.set_active_block_metadata(
                BlockMetadata::new(Some(SessionId::from(0)), Some("~".into())),
                false,
                ctx,
            )
        });

        // Abbreviations are expanded and take priority over aliases
        input.update(&mut app, |input, ctx| {
            input.user_insert("g ", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Cursor is now at "g |"
        });
        input.update(&mut app, |input, ctx| {
            input.run_expansion_on_space(ctx);
            assert_eq!(input.buffer_text(ctx), "git log ");
        });
    });
}

#[test]
fn test_alias_expansion_when_alias_expansion_is_disabled() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let abbreviations = HashMap::from_iter([("gco".into(), "git checkout".into())]);
        let aliases =
            HashMap::from_iter([("g".into(), "git".into()), ("vi".into(), "nvim".into())]);
        let session_info = SessionInfo::new_for_test()
            .with_aliases(aliases)
            .with_abbreviations(abbreviations);

        set_alias_expansion_setting(false, &mut app);
        let terminal = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;

        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let editor = input.read(&app, |input, _| input.editor().clone());

        input.update(&mut app, |input, ctx| {
            input.set_active_block_metadata(
                BlockMetadata::new(Some(SessionId::from(0)), Some("~".into())),
                false,
                ctx,
            )
        });

        // Aliases are not expanded
        input.update(&mut app, |input, ctx| {
            input.user_insert("g ", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Cursor is now at "g |"
        });
        input.update(&mut app, |input, ctx| {
            input.run_expansion_on_space(ctx);
            assert_eq!(input.buffer_text(ctx), "g ");
        });

        // Abbreviations are still expanded
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("gco ", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Cursor is now at "gco |"
        });
        input.update(&mut app, |input, ctx| {
            input.run_expansion_on_space(ctx);
            assert_eq!(input.buffer_text(ctx), "git checkout ");
        });
    });
}

#[test]
fn test_get_expanded_command_on_execute() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let aliases = HashMap::from_iter([("gco".into(), "git checkout".into())]);
        let session_info = SessionInfo::new_for_test().with_aliases(aliases);

        set_alias_expansion_setting(true, &mut app);
        let terminal = add_window_with_bootstrapped_terminal(
            &mut app,
            None, /* history_file_commands */
            Some(session_info),
        )
        .await;

        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let editor = input.read(&app, |input, _| input.editor().clone());

        input.update(&mut app, |input, ctx| {
            input.set_active_block_metadata(
                BlockMetadata::new(Some(SessionId::from(0)), Some("~".into())),
                false,
                ctx,
            )
        });

        // Expansion happens at the end of the line
        input.update(&mut app, |input, ctx| {
            input.user_insert("gco", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Cursor is now at "gco|"
        });
        input.update(&mut app, |input, ctx| {
            let result = input.get_expanded_command_on_execute(ctx);
            assert_eq!(result, Some("git checkout".into()));
        });

        // Commands are expanded when cursor is in middle of the line
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("gco test", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            use crate::editor::EditorAction;
            editor.move_to_buffer_start(ctx);
            editor.handle_action(&EditorAction::MoveForwardOneWord, ctx);
            // Cursor is now at "gco| test"
        });
        input.update(&mut app, |input, ctx| {
            let result = input.get_expanded_command_on_execute(ctx);
            assert_eq!(result, Some("git checkout test".into()));
        });

        // Returns None if there is no alias to be expanded.
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("echo Hello", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            // Cursor is now at "echo Hello|"
        });
        input.update(&mut app, |input, ctx| {
            let result = input.get_expanded_command_on_execute(ctx);
            assert_eq!(result, None);
        });
    });
}

#[test]
fn test_tab_completions_menu_for_regular_completions() {
    let _flag = FeatureFlag::ClassicCompletions.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("cd Do", ctx);
        });

        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![file_suggestion("Downloads"), file_suggestion("Documents")],
                    (3, 5),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });

        let expected_menu_position = TabCompletionsMenuPosition::AtLastCursor;
        input.read(&app, |input, ctx| {
            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::CompletionSuggestions { menu_position, .. } if menu_position == &expected_menu_position
            ))
        });
    })
}

#[test]
fn test_tab_completions_menu_for_classic_completions() {
    let _flag = FeatureFlag::ClassicCompletions.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        app.update(|ctx| {
            InputSettings::handle(ctx).update(ctx, |setting, ctx| {
                setting
                    .classic_completions_mode
                    .toggle_and_save_value(ctx)
                    .expect("Able to turn on classic completions");
            })
        });

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("cd Do", ctx);
        });

        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![file_suggestion("Downloads"), file_suggestion("Documents")],
                    (3, 5),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });

        input.read(&app, |input, ctx| {
            // The menu should be docked after `cd `.
            assert_eq!(
                input.editor.as_ref(ctx).get_cached_buffer_point(COMPLETIONS_START_OF_REPLACEMENT_SPAN_POSITION_ID),
                Some(Point { row: 0, column: 3 })
            );
            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::CompletionSuggestions { menu_position, .. } if menu_position == &TabCompletionsMenuPosition::AtStartOfReplacementSpan
            ))
        });
    })
}

#[test]
fn test_tab_completions_menu_for_classic_completions_with_files() {
    let _flag = FeatureFlag::ClassicCompletions.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        app.update(|ctx| {
            InputSettings::handle(ctx).update(ctx, |setting, ctx| {
                setting
                    .classic_completions_mode
                    .toggle_and_save_value(ctx)
                    .expect("Able to turn on classic completions");
            })
        });

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("cd foo/Do", ctx);
        });

        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        file_suggestion("foo/Downloads"),
                        file_suggestion("foo/Documents"),
                    ],
                    (3, 9),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });

        input.read(&app, |input, ctx| {
            // The menu should be docked after `cd foo/`.
            assert_eq!(
                input.editor.as_ref(ctx).get_cached_buffer_point(COMPLETIONS_START_OF_REPLACEMENT_SPAN_POSITION_ID),
                Some(Point { row: 0, column: 7 })
            );
            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::CompletionSuggestions { menu_position, .. } if menu_position == &TabCompletionsMenuPosition::AtStartOfReplacementSpan
            ))
        });
    })
}

#[test]
fn test_classic_tab_completions_close_after_user_backspace() {
    let _flag = FeatureFlag::ClassicCompletions.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let editor = input.read(&app, |input, _| input.editor().clone());

        app.update(|ctx| {
            InputSettings::handle(ctx).update(ctx, |setting, ctx| {
                setting
                    .classic_completions_mode
                    .toggle_and_save_value(ctx)
                    .expect("Able to turn on classic completions");
            })
        });

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("cd Do", ctx);
        });

        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![file_suggestion("Downloads"), file_suggestion("Documents")],
                    (3, 5),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
            // Cycle to apply a candidate into the buffer. This is a system-applied
            // edit, which must keep the result set alive.
            input.input_tab(ctx);
        });

        // The user now backspaces all the way past the original completion query
        // (`cd Do`). Once the buffer no longer starts with the original query, the
        // stale result set must be discarded and the menu closed.
        while input.read(&app, |input, ctx| input.buffer_text(ctx).len()) > "cd ".len() {
            editor.update(&mut app, |editor, ctx| editor.backspace(ctx));
        }

        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd ");
            // A closed menu is represented by `InputSuggestionsMode::Closed`; a closed
            // menu is never rendered, so its stale result set is no longer shown. This
            // mirrors the existing (non-classic) backspace-past-boundary behavior.
            assert!(
                matches!(
                    input.suggestions_mode_model.as_ref(ctx).mode(),
                    InputSuggestionsMode::Closed
                ),
                "completion menu should close after the user backspaces past the query"
            );
        });
    })
}

#[test]
fn test_classic_tab_completions_keep_menu_open_while_cycling() {
    let _flag = FeatureFlag::ClassicCompletions.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        app.update(|ctx| {
            InputSettings::handle(ctx).update(ctx, |setting, ctx| {
                setting
                    .classic_completions_mode
                    .toggle_and_save_value(ctx)
                    .expect("Able to turn on classic completions");
            })
        });

        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("cd Do", ctx);
        });

        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![file_suggestion("Downloads"), file_suggestion("Documents")],
                    (3, 5),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
            // Cycling rewrites the buffer to each candidate in turn. These are
            // system-applied edits and must keep the menu open even though the
            // buffer no longer matches the original query.
            input.input_tab(ctx);
            input.input_tab(ctx);
        });

        input.read(&app, |input, ctx| {
            assert!(
                matches!(
                    input.suggestions_mode_model.as_ref(ctx).mode(),
                    InputSuggestionsMode::CompletionSuggestions { .. }
                ),
                "completion menu should stay open while cycling candidates"
            );
            assert!(
                !input.input_suggestions.as_ref(ctx).items().is_empty(),
                "result set should be preserved while cycling candidates"
            );
        });
    })
}

#[test]
fn test_vim_escape_with_history_menu() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        enable_vim_mode(&mut app);
        let history_file_commands = vec!["cd ~".to_string(), "ls".to_string()];
        let terminal =
            add_window_with_bootstrapped_terminal(&mut app, Some(history_file_commands), None)
                .await;
        let (input, editor) = terminal.read(&app, |view, ctx| {
            let input = view.input().clone();
            let editor = input.as_ref(ctx).editor().clone();
            (input, editor)
        });

        // Arrow up displays history in the correct order for an empty buffer
        input.update(&mut app, |input, ctx| {
            input.editor_up(ctx);
        });
        input.read(&app, |input, ctx| {
            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::HistoryUp { .. }
            ));
        });

        // If input suggestions are history, Esc key should exit normal mode before dismissing the
        // history menu.
        editor.update(&mut app, |editor, ctx| {
            assert_eq!(editor.vim_mode(ctx), Some(VimMode::Insert));
            editor.escape(ctx);
        });
        editor.read(&app, |editor, ctx| {
            assert_eq!(editor.vim_mode(ctx), Some(VimMode::Normal));
        });
        input.read(&app, |input, ctx| {
            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::HistoryUp { .. }
            ));
        });

        editor.update(&mut app, |editor, ctx| {
            editor.escape(ctx);
        });
        editor.read(&app, |editor, ctx| {
            assert_eq!(editor.vim_mode(ctx), Some(VimMode::Normal));
        });
        input.read(&app, |input, ctx| {
            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::Closed
            ));
        });
    });
}

#[test]
fn test_vim_escape_with_completions() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        enable_vim_mode(&mut app);
        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;

        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let editor = input.read(&app, |input, _| input.editor().clone());

        editor.read(&app, |editor, ctx| {
            assert_eq!(editor.vim_mode(ctx), Some(VimMode::Insert));
        });
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert("c", ctx);
            input.user_insert("d", ctx);
            input.user_insert(" ", ctx);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd ");
        });
        input.update(&mut app, |input, ctx| {
            input.input_tab(ctx);
            input.handle_completion_suggestions_results(
                build_suggestion_results(
                    vec![
                        argument_suggestion("Documents"),
                        argument_suggestion("Pictures"),
                    ],
                    (3, 3),
                    MatchStrategy::CaseInsensitive,
                ),
                CompletionsTrigger::Keybinding,
                editor_model_snapshot(input, ctx),
                ctx,
            );
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cd ");
            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::CompletionSuggestions { .. }
            ));
        });

        // If input suggestions are completions, Esc key should dismiss that before exiting normal
        // mode.
        editor.update(&mut app, |editor, ctx| {
            editor.escape(ctx);
        });
        editor.read(&app, |editor, ctx| {
            assert_eq!(editor.vim_mode(ctx), Some(VimMode::Insert));
        });
        input.read(&app, |input, ctx| {
            assert!(matches!(
                input.suggestions_mode_model.as_ref(ctx).mode(),
                InputSuggestionsMode::Closed
            ));
        });

        editor.update(&mut app, |editor, ctx| {
            editor.escape(ctx);
        });
        editor.read(&app, |editor, ctx| {
            assert_eq!(editor.vim_mode(ctx), Some(VimMode::Normal));
        });
    });
}

#[test]
fn test_should_show_completions_in_ai_input() {
    // Test cases where the function should return true
    // i.e. we should trigger completions-as-you-type in AI input.
    assert!(should_show_completions_in_ai_input("/"));
    assert!(should_show_completions_in_ai_input("/foo"));
    assert!(should_show_completions_in_ai_input("some text /foo"));

    assert!(should_show_completions_in_ai_input("./"));
    assert!(should_show_completions_in_ai_input("./foo"));
    assert!(should_show_completions_in_ai_input("some text ./foo"));

    assert!(should_show_completions_in_ai_input("foo/"));
    assert!(should_show_completions_in_ai_input("~/"));
    assert!(should_show_completions_in_ai_input("foo/bar"));
    assert!(should_show_completions_in_ai_input("some text foo/bar"));

    assert!(should_show_completions_in_ai_input("../"));
    assert!(should_show_completions_in_ai_input("../foo"));
    assert!(should_show_completions_in_ai_input("bar ../foo"));

    // Test cases where the function should return false
    // i.e. we should NOT trigger completions-as-you-type in AI input.
    assert!(!should_show_completions_in_ai_input("foo"));
    assert!(!should_show_completions_in_ai_input("some text/ foo"));
    assert!(!should_show_completions_in_ai_input("./bar foo"));
    assert!(!should_show_completions_in_ai_input("some text / bar foo"));
    assert!(!should_show_completions_in_ai_input(""));
    assert!(!should_show_completions_in_ai_input("../foo bar"));
    // Space at the end invalidates triggering completions.
    assert!(!should_show_completions_in_ai_input("../foo "));
}

#[test]
fn test_remove_ignored_suggestion_on_command_execution() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;
        let input = terminal.read(&app, |view, _| view.input().clone());

        // First, add a command to ignored suggestions
        let test_command = "echo hi";
        IgnoredSuggestionsModel::handle(&app).update(&mut app, |model, ctx| {
            model.add_ignored_suggestion(
                test_command.to_string(),
                crate::suggestions::ignored_suggestions_model::SuggestionType::ShellCommand,
                ctx,
            );
        });

        // Verify the command is ignored
        let is_ignored_before = IgnoredSuggestionsModel::handle(&app).read(&app, |model, _| {
            model.is_ignored(
                test_command,
                crate::suggestions::ignored_suggestions_model::SuggestionType::ShellCommand,
            )
        });
        assert!(is_ignored_before, "Command should be ignored initially");

        // Execute the command
        input.update(&mut app, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
            input.user_insert(test_command, ctx);
            input.try_execute_command(test_command, ctx);
        });

        // Verify the command is no longer ignored
        let is_ignored_after = IgnoredSuggestionsModel::handle(&app).read(&app, |model, _| {
            model.is_ignored(
                test_command,
                crate::suggestions::ignored_suggestions_model::SuggestionType::ShellCommand,
            )
        });
        assert!(
            !is_ignored_after,
            "Command should no longer be ignored after execution"
        );
    });
}

#[test]
fn test_page_up_and_down_scroll_terminal_from_prompt() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;
        let (input, editor) = terminal.read(&app, |terminal, ctx| {
            let input = terminal.input().clone();
            let editor = input.as_ref(ctx).editor().clone();
            (input, editor)
        });

        terminal.update(&mut app, |terminal, _| {
            terminal
                .model
                .lock()
                .simulate_block("ls", &"\n".repeat(1000));
        });

        input.update(&mut app, |input, ctx| {
            input.user_insert("echo first line\necho second line", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.move_to_buffer_end(ctx);
            editor.handle_action(&EditorAction::PageUp, ctx);
        });

        assert_eq!(
            input.read(&app, |input, ctx| input.buffer_text(ctx)),
            "echo first line\necho second line"
        );
        let scroll_position_after_page_up =
            terminal.read(&app, |terminal, _| terminal.scroll_position());
        assert!(matches!(
            scroll_position_after_page_up,
            ScrollPosition::FixedAtPosition { .. }
        ));

        editor.update(&mut app, |editor, ctx| {
            editor.handle_action(&EditorAction::PageDown, ctx);
        });

        assert_eq!(
            input.read(&app, |input, ctx| input.buffer_text(ctx)),
            "echo first line\necho second line"
        );
        let scroll_position_after_page_down =
            terminal.read(&app, |terminal, _| terminal.scroll_position());
        assert_ne!(
            scroll_position_after_page_down,
            scroll_position_after_page_up
        );
    });
}

#[test]
fn test_page_up_and_down_do_not_scroll_terminal_when_suggestions_are_visible() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let history_file_commands = vec![
            "echo alpha\necho beta".to_string(),
            "git status\ngit diff".to_string(),
        ];
        let terminal =
            add_window_with_bootstrapped_terminal(&mut app, Some(history_file_commands), None)
                .await;
        let (input, editor) = terminal.read(&app, |terminal, ctx| {
            let input = terminal.input().clone();
            let editor = input.as_ref(ctx).editor().clone();
            (input, editor)
        });

        terminal.update(&mut app, |terminal, _| {
            terminal
                .model
                .lock()
                .simulate_block("ls", &"\n".repeat(1000));
        });

        input.update(&mut app, |input, ctx| {
            input.handle_action(&InputAction::Up, ctx);
            assert!(input.suggestions_mode_model.as_ref(ctx).is_visible());
        });

        let initial_scroll_position = terminal.read(&app, |terminal, _| terminal.scroll_position());
        let initial_buffer = input.read(&app, |input, ctx| input.buffer_text(ctx));

        editor.update(&mut app, |editor, ctx| {
            editor.handle_action(&EditorAction::PageUp, ctx);
            editor.handle_action(&EditorAction::PageDown, ctx);
        });

        terminal.read(&app, |terminal, _| {
            assert_eq!(terminal.scroll_position(), initial_scroll_position);
        });
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), initial_buffer);
            assert!(input.suggestions_mode_model.as_ref(ctx).is_visible());
        });
    });
}

#[test]
fn test_page_up_and_down_scroll_terminal_with_vim_mode_enabled() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;
        let (input, editor) = terminal.read(&app, |terminal, ctx| {
            let input = terminal.input().clone();
            let editor = input.as_ref(ctx).editor().clone();
            (input, editor)
        });

        terminal.update(&mut app, |terminal, _| {
            terminal
                .model
                .lock()
                .simulate_block("ls", &"\n".repeat(1000));
        });

        AppEditorSettings::handle(&app).update(&mut app, |settings, settings_ctx| {
            let _ = settings.vim_mode.set_value(true, settings_ctx);
        });

        input.update(&mut app, |input, ctx| {
            input.user_insert("echo first line\necho second line", ctx);
        });
        editor.update(&mut app, |editor, ctx| {
            editor.vim_keystroke(&Keystroke::parse("escape").unwrap(), ctx);
        });
        editor.read(&app, |editor, ctx| {
            assert_eq!(editor.vim_mode(ctx), Some(VimMode::Normal));
        });

        editor.update(&mut app, |editor, ctx| {
            editor.handle_action(&EditorAction::PageUp, ctx);
        });

        assert_eq!(
            input.read(&app, |input, ctx| input.buffer_text(ctx)),
            "echo first line\necho second line"
        );
        let scroll_position_after_page_up =
            terminal.read(&app, |terminal, _| terminal.scroll_position());
        assert!(matches!(
            scroll_position_after_page_up,
            ScrollPosition::FixedAtPosition { .. }
        ));

        editor.update(&mut app, |editor, ctx| {
            editor.handle_action(&EditorAction::PageDown, ctx);
        });

        assert_eq!(
            input.read(&app, |input, ctx| input.buffer_text(ctx)),
            "echo first line\necho second line"
        );
        let scroll_position_after_page_down =
            terminal.read(&app, |terminal, _| terminal.scroll_position());
        assert_ne!(
            scroll_position_after_page_down,
            scroll_position_after_page_up
        );
    });
}

#[test]
fn test_custom_terminal_page_scroll_binding_applies_when_prompt_is_focused() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let (window_id, terminal) =
            add_window_with_bootstrapped_terminal_and_window_id(&mut app, None, None).await;
        let (input, editor) = terminal.read(&app, |terminal, ctx| {
            let input = terminal.input().clone();
            let editor = input.as_ref(ctx).editor().clone();
            (input, editor)
        });

        terminal.update(&mut app, |terminal, _| {
            terminal
                .model
                .lock()
                .simulate_block("ls", &"\n".repeat(1000));
        });

        app.update(|ctx| {
            ctx.set_custom_trigger(
                "terminal:scroll_up_one_page".to_owned(),
                warpui::keymap::Trigger::Keystrokes(vec![
                    Keystroke::parse("shift-pageup").unwrap(),
                ]),
            );
        });

        let focus_path = [terminal.id(), input.id(), editor.id()];

        let handled = app
            .dispatch_keystroke(
                window_id,
                &focus_path,
                &Keystroke::parse("pageup").unwrap(),
                false,
            )
            .unwrap();
        assert!(!handled);
        terminal.read(&app, |terminal, _| {
            assert_eq!(
                terminal.scroll_position(),
                ScrollPosition::FollowsBottomOfMostRecentBlock
            );
        });

        let handled = app
            .dispatch_keystroke(
                window_id,
                &focus_path,
                &Keystroke::parse("shift-pageup").unwrap(),
                false,
            )
            .unwrap();
        assert!(handled);
        terminal.read(&app, |terminal, _| {
            assert!(matches!(
                terminal.scroll_position(),
                ScrollPosition::FixedAtPosition { .. }
            ));
        });
    });
}
