use std::sync::Arc;

use parking_lot::FairMutex;
use session_sharing_protocol::common::{
    OrderedTerminalEvent, OrderedTerminalEventType, Scrollback, ScrollbackBlock, WindowSize,
};
use warp_core::command::ExitCode;
use warp_core::features::FeatureFlag;
use warpui::units::Lines;
use warpui::{App, ViewHandle};

use crate::terminal::TerminalView;
use crate::terminal::event_listener::ChannelEventListener;
use crate::terminal::model::block::{BlockId, BlockState, SerializedBlock};
use crate::terminal::shared_session::shared_handlers::RemoteUpdateGuard;
use crate::terminal::shared_session::tests::terminal_model_for_viewer;
use crate::terminal::shared_session::viewer::event_loop::{
    EventLoop, SharedSessionInitialLoadMode,
};
use crate::test_util::add_window_with_terminal;
use crate::test_util::terminal::initialize_app_for_terminal_view;

fn ordered_terminal_event_from_bytes(
    bytes: impl Into<Vec<u8>>,
    event_no: usize,
) -> OrderedTerminalEvent {
    let compressed = lz4_flex::block::compress_prepend_size(&bytes.into());
    OrderedTerminalEvent {
        event_no,
        event_type: OrderedTerminalEventType::PtyBytesRead { bytes: compressed },
    }
}

fn old_sharer_dcs_bytes(payload: &str) -> Vec<u8> {
    let mut bytes = b"\x1bP$d".to_vec();
    bytes.extend(hex::encode(payload).bytes());
    bytes.push(0x9c);
    bytes
}

fn terminal_view(app: &mut App) -> ViewHandle<TerminalView> {
    initialize_app_for_terminal_view(app);
    add_window_with_terminal(app, None)
}

fn completed_block(command: &str, output: &str) -> SerializedBlock {
    let mut block =
        SerializedBlock::new_for_test(command.as_bytes().into(), output.as_bytes().into());
    block.id = BlockId::new();
    block
}

fn active_block() -> SerializedBlock {
    let mut block = SerializedBlock::new_active_block_for_test();
    block.id = BlockId::new();
    block
}

fn scrollback_block(block: &SerializedBlock) -> ScrollbackBlock {
    ScrollbackBlock {
        raw: serde_json::to_vec(block).unwrap(),
    }
}

fn empty_scrollback() -> Scrollback {
    Scrollback {
        blocks: vec![],
        is_alt_screen_active: false,
    }
}

#[test]
fn test_terminal_model_is_correct() {
    App::test((), |mut app| async move {
        let channel_event_proxy = ChannelEventListener::new_for_test();
        let model = Arc::new(FairMutex::new(terminal_model_for_viewer(
            channel_event_proxy.clone(),
        )));

        let terminal_view = terminal_view(&mut app);
        let event_loop = app.add_model(|ctx| {
            EventLoop::new(
                model.clone(),
                terminal_view.downgrade(),
                channel_event_proxy.clone(),
                WindowSize {
                    num_rows: 0,
                    num_cols: 0,
                },
                Scrollback {
                    blocks: vec![],
                    is_alt_screen_active: false,
                },
                None,
                SharedSessionInitialLoadMode::ReplaceFromSessionScrollback,
                RemoteUpdateGuard::new(),
                ctx,
            )
        });

        // Before we receive any events, the block list only contains hidden blocks.
        assert!(model.lock().block_list().blocks().iter().all(|block| {
            block.height(&crate::terminal::model::block::TranscriptScope::Terminal) == Lines::zero()
        }));

        // Load shared session scrollback.
        let scrollback = &[
            SerializedBlock::new_for_test("block1".into(), "block1".into()),
            SerializedBlock::new_active_block_for_test(),
        ];
        {
            let mut model = model.lock();
            model.load_shared_session_scrollback(scrollback);
            // A hidden block, a completed scrollback block, then the active block.
            assert_eq!(model.block_list().blocks().len(), 3);
            assert_eq!(
                model.block_list().blocks()[0]
                    .height(&crate::terminal::model::block::TranscriptScope::Terminal),
                Lines::zero()
            );
            assert_ne!(
                model.block_list().blocks()[1]
                    .height(&crate::terminal::model::block::TranscriptScope::Terminal),
                Lines::zero()
            );
            assert_eq!(
                model.block_list().blocks()[2]
                    .height(&crate::terminal::model::block::TranscriptScope::Terminal),
                Lines::zero()
            );
        }

        // Write some PTY events after starting active block.
        model.lock().start_command_execution();
        event_loop.update(&mut app, |event_loop, ctx| {
            event_loop
                .process_ordered_terminal_event(ordered_terminal_event_from_bytes("a", 0), ctx);
        });

        let model = model.lock();
        // After writing bytes, active block should no longer have height 0.
        assert_eq!(model.block_list().blocks().len(), 3);
        assert_eq!(
            model.block_list().blocks()[0]
                .height(&crate::terminal::model::block::TranscriptScope::Terminal),
            Lines::zero()
        );
        assert_ne!(
            model.block_list().blocks()[1]
                .height(&crate::terminal::model::block::TranscriptScope::Terminal),
            Lines::zero()
        );
        assert_ne!(
            model.block_list().blocks()[2]
                .height(&crate::terminal::model::block::TranscriptScope::Terminal),
            Lines::zero()
        );
    })
}

#[test]
fn new_viewer_processes_old_sharer_lifecycle_stream() {
    let _recovery_enabled = FeatureFlag::TerminalLifecycleRecovery.override_enabled(true);
    App::test((), |mut app| async move {
        let channel_event_proxy = ChannelEventListener::new_for_test();
        let model = Arc::new(FairMutex::new(terminal_model_for_viewer(
            channel_event_proxy.clone(),
        )));
        let terminal_view = terminal_view(&mut app);
        let event_loop = app.add_model(|ctx| {
            EventLoop::new(
                model.clone(),
                terminal_view.downgrade(),
                channel_event_proxy.clone(),
                WindowSize {
                    num_rows: 0,
                    num_cols: 0,
                },
                empty_scrollback(),
                None,
                SharedSessionInitialLoadMode::ReplaceFromSessionScrollback,
                RemoteUpdateGuard::new(),
                ctx,
            )
        });

        let completed_block_id = model.lock().active_block_id().clone();
        let next_block_id = BlockId::new();
        let command_finished = old_sharer_dcs_bytes(&format!(
            r#"{{"hook":"CommandFinished","value":{{"exit_code":47,"next_block_id":"{next_block_id}","session_id":987654321}}}}"#
        ));
        let precmd = old_sharer_dcs_bytes(
            r#"{"hook":"Precmd","value":{"pwd":"/old-sharer","session_id":987654321}}"#,
        );

        event_loop.update(&mut app, |event_loop, ctx| {
            event_loop.process_ordered_terminal_event(
                OrderedTerminalEvent {
                    event_no: 0,
                    event_type: OrderedTerminalEventType::CommandExecutionStarted {
                        participant_id: Default::default(),
                        ai_metadata: None,
                    },
                },
                ctx,
            );
            event_loop.process_ordered_terminal_event(
                ordered_terminal_event_from_bytes(command_finished, 1),
                ctx,
            );
            event_loop.process_ordered_terminal_event(
                OrderedTerminalEvent {
                    event_no: 2,
                    event_type: OrderedTerminalEventType::CommandExecutionFinished {
                        next_block_id: next_block_id.to_string().into(),
                    },
                },
                ctx,
            );
            event_loop
                .process_ordered_terminal_event(ordered_terminal_event_from_bytes(precmd, 3), ctx);
        });

        let model = model.lock();
        let completed_block = model
            .block_list()
            .block_with_id(&completed_block_id)
            .expect("The old sharer's completed block should remain in the block list.");
        assert_eq!(completed_block.state(), BlockState::DoneWithExecution);
        assert_eq!(completed_block.exit_code(), ExitCode::from(47));
        assert_eq!(
            model
                .block_list()
                .blocks()
                .iter()
                .filter(|block| block.state() == BlockState::DoneWithExecution)
                .count(),
            1
        );
        assert_eq!(model.active_block_id(), &next_block_id);
        assert_eq!(
            model.block_list().active_block().pwd().map(String::as_str),
            Some("/old-sharer")
        );
        assert_eq!(
            model.block_list().active_block().state(),
            BlockState::BeforeExecution
        );
    })
}

#[test]
fn test_append_followup_scrollback_skips_duplicates() {
    App::test((), |mut app| async move {
        let channel_event_proxy = ChannelEventListener::new_for_test();
        let model = Arc::new(FairMutex::new(terminal_model_for_viewer(
            channel_event_proxy.clone(),
        )));

        let terminal_view = terminal_view(&mut app);
        let initial_completed = completed_block("initial-command", "initial-output");
        let initial_active = active_block();
        app.add_model(|ctx| {
            EventLoop::new(
                model.clone(),
                terminal_view.downgrade(),
                channel_event_proxy.clone(),
                WindowSize {
                    num_rows: 0,
                    num_cols: 0,
                },
                Scrollback {
                    blocks: vec![
                        scrollback_block(&initial_completed),
                        scrollback_block(&initial_active),
                    ],
                    is_alt_screen_active: false,
                },
                None,
                SharedSessionInitialLoadMode::ReplaceFromSessionScrollback,
                RemoteUpdateGuard::new(),
                ctx,
            )
        });

        assert_eq!(model.lock().block_list().blocks().len(), 3);

        let followup_completed = completed_block("followup-command", "followup-output");
        let followup_active = active_block();
        app.add_model(|ctx| {
            EventLoop::new(
                model.clone(),
                terminal_view.downgrade(),
                channel_event_proxy.clone(),
                WindowSize {
                    num_rows: 0,
                    num_cols: 0,
                },
                Scrollback {
                    blocks: vec![
                        scrollback_block(&initial_completed),
                        scrollback_block(&followup_completed),
                        scrollback_block(&followup_active),
                    ],
                    is_alt_screen_active: false,
                },
                None,
                SharedSessionInitialLoadMode::AppendFollowupScrollback,
                RemoteUpdateGuard::new(),
                ctx,
            )
        });

        {
            let model = model.lock();
            let commands = model
                .block_list()
                .blocks()
                .iter()
                .map(|block| block.command_to_string())
                .collect::<Vec<_>>();
            assert_eq!(model.block_list().blocks().len(), 5);
            assert_eq!(
                commands
                    .iter()
                    .filter(|command| command.as_str() == "initial-command")
                    .count(),
                1
            );
            assert_eq!(
                commands
                    .iter()
                    .filter(|command| command.as_str() == "followup-command")
                    .count(),
                1
            );
        }

        let second_followup_completed =
            completed_block("second-followup-command", "second-followup-output");
        let second_followup_active = active_block();
        app.add_model(|ctx| {
            EventLoop::new(
                model.clone(),
                terminal_view.downgrade(),
                channel_event_proxy.clone(),
                WindowSize {
                    num_rows: 0,
                    num_cols: 0,
                },
                Scrollback {
                    blocks: vec![
                        scrollback_block(&initial_completed),
                        scrollback_block(&followup_completed),
                        scrollback_block(&second_followup_completed),
                        scrollback_block(&second_followup_active),
                    ],
                    is_alt_screen_active: false,
                },
                None,
                SharedSessionInitialLoadMode::AppendFollowupScrollback,
                RemoteUpdateGuard::new(),
                ctx,
            )
        });

        let model = model.lock();
        let commands = model
            .block_list()
            .blocks()
            .iter()
            .map(|block| block.command_to_string())
            .collect::<Vec<_>>();
        assert_eq!(model.block_list().blocks().len(), 7);
        for command in [
            "initial-command",
            "followup-command",
            "second-followup-command",
        ] {
            assert_eq!(
                commands
                    .iter()
                    .filter(|existing_command| existing_command.as_str() == command)
                    .count(),
                1
            );
        }
    })
}

#[test]
fn test_append_followup_scrollback_with_completed_last_block_creates_active_block() {
    App::test((), |mut app| async move {
        let channel_event_proxy = ChannelEventListener::new_for_test();
        let model = Arc::new(FairMutex::new(terminal_model_for_viewer(
            channel_event_proxy.clone(),
        )));

        let terminal_view = terminal_view(&mut app);
        let initial_completed = completed_block("initial-command", "initial-output");
        let initial_active = active_block();
        app.add_model(|ctx| {
            EventLoop::new(
                model.clone(),
                terminal_view.downgrade(),
                channel_event_proxy.clone(),
                WindowSize {
                    num_rows: 0,
                    num_cols: 0,
                },
                Scrollback {
                    blocks: vec![
                        scrollback_block(&initial_completed),
                        scrollback_block(&initial_active),
                    ],
                    is_alt_screen_active: false,
                },
                None,
                SharedSessionInitialLoadMode::ReplaceFromSessionScrollback,
                RemoteUpdateGuard::new(),
                ctx,
            )
        });

        let followup_completed = completed_block("followup-command", "followup-output");
        app.add_model(|ctx| {
            EventLoop::new(
                model.clone(),
                terminal_view.downgrade(),
                channel_event_proxy.clone(),
                WindowSize {
                    num_rows: 0,
                    num_cols: 0,
                },
                Scrollback {
                    blocks: vec![
                        scrollback_block(&initial_completed),
                        scrollback_block(&followup_completed),
                    ],
                    is_alt_screen_active: false,
                },
                None,
                SharedSessionInitialLoadMode::AppendFollowupScrollback,
                RemoteUpdateGuard::new(),
                ctx,
            )
        });

        let model = model.lock();
        let commands = model
            .block_list()
            .blocks()
            .iter()
            .map(|block| block.command_to_string())
            .collect::<Vec<_>>();
        assert_eq!(model.block_list().blocks().len(), 5);
        assert_eq!(
            commands
                .iter()
                .filter(|command| command.as_str() == "initial-command")
                .count(),
            1
        );
        assert_eq!(
            commands
                .iter()
                .filter(|command| command.as_str() == "followup-command")
                .count(),
            1
        );
        assert_eq!(model.block_list().active_block_index(), 4.into());
        assert_eq!(
            model
                .block_list()
                .active_block()
                .height(&crate::terminal::model::block::TranscriptScope::Terminal),
            Lines::zero()
        );
        assert!(!model.block_list().active_block().started());
    })
}

#[test]
fn test_append_followup_replay_marks_existing_conversations_suppressible() {
    App::test((), |mut app| async move {
        let channel_event_proxy = ChannelEventListener::new_for_test();
        let model = Arc::new(FairMutex::new(terminal_model_for_viewer(
            channel_event_proxy.clone(),
        )));

        let terminal_view = terminal_view(&mut app);
        let event_loop = app.add_model(|ctx| {
            EventLoop::new(
                model.clone(),
                terminal_view.downgrade(),
                channel_event_proxy.clone(),
                WindowSize {
                    num_rows: 0,
                    num_cols: 0,
                },
                empty_scrollback(),
                None,
                SharedSessionInitialLoadMode::AppendFollowupScrollback,
                RemoteUpdateGuard::new(),
                ctx,
            )
        });

        event_loop.update(&mut app, |event_loop, ctx| {
            event_loop.process_ordered_terminal_event(
                OrderedTerminalEvent {
                    event_no: 0,
                    event_type: OrderedTerminalEventType::AgentConversationReplayStarted,
                },
                ctx,
            );
        });

        {
            let model = model.lock();
            assert!(model.is_receiving_agent_conversation_replay());
        }

        event_loop.update(&mut app, |event_loop, ctx| {
            event_loop.process_ordered_terminal_event(
                OrderedTerminalEvent {
                    event_no: 1,
                    event_type: OrderedTerminalEventType::AgentConversationReplayEnded,
                },
                ctx,
            );
        });

        let model = model.lock();
        assert!(!model.is_receiving_agent_conversation_replay());
    })
}

#[test]
fn test_fresh_session_replay_does_not_suppress_existing_conversations() {
    App::test((), |mut app| async move {
        let channel_event_proxy = ChannelEventListener::new_for_test();
        let model = Arc::new(FairMutex::new(terminal_model_for_viewer(
            channel_event_proxy.clone(),
        )));

        let terminal_view = terminal_view(&mut app);
        let event_loop = app.add_model(|ctx| {
            EventLoop::new(
                model.clone(),
                terminal_view.downgrade(),
                channel_event_proxy.clone(),
                WindowSize {
                    num_rows: 0,
                    num_cols: 0,
                },
                empty_scrollback(),
                None,
                SharedSessionInitialLoadMode::ReplaceFromSessionScrollback,
                RemoteUpdateGuard::new(),
                ctx,
            )
        });

        event_loop.update(&mut app, |event_loop, ctx| {
            event_loop.process_ordered_terminal_event(
                OrderedTerminalEvent {
                    event_no: 0,
                    event_type: OrderedTerminalEventType::AgentConversationReplayStarted,
                },
                ctx,
            );
        });

        let model = model.lock();
        assert!(model.is_receiving_agent_conversation_replay());
    })
}

#[test]
fn test_out_of_order_buffering() {
    App::test((), |mut app| async move {
        let channel_event_proxy = ChannelEventListener::new_for_test();
        let model = Arc::new(FairMutex::new(terminal_model_for_viewer(
            channel_event_proxy.clone(),
        )));

        let terminal_view = terminal_view(&mut app);
        let active_block: SerializedBlock = model.lock().block_list().active_block().into();
        let event_loop = app.add_model(|ctx| {
            EventLoop::new(
                model.clone(),
                terminal_view.downgrade(),
                channel_event_proxy.clone(),
                WindowSize {
                    num_rows: 0,
                    num_cols: 0,
                },
                Scrollback {
                    blocks: vec![ScrollbackBlock {
                        raw: serde_json::to_vec(&active_block).unwrap(),
                    }],
                    is_alt_screen_active: false,
                },
                None,
                SharedSessionInitialLoadMode::ReplaceFromSessionScrollback,
                RemoteUpdateGuard::new(),
                ctx,
            )
        });

        // Simulate the real event flow: CommandExecutionStarted (event_no 0) arrives first,
        // then PTY bytes (event_no 1-3) potentially in out-of-order sequence.
        event_loop.update(&mut app, |event_loop, ctx| {
            // First: sharer sends CommandExecutionStarted when user executes a command
            event_loop.process_ordered_terminal_event(
                OrderedTerminalEvent {
                    event_no: 0,
                    event_type: OrderedTerminalEventType::CommandExecutionStarted {
                        participant_id: Default::default(),
                        ai_metadata: None,
                    },
                },
                ctx,
            );

            // Then: PTY bytes arrive (potentially out of order)
            event_loop
                .process_ordered_terminal_event(ordered_terminal_event_from_bytes("c", 3), ctx);
            event_loop
                .process_ordered_terminal_event(ordered_terminal_event_from_bytes("b", 2), ctx);
            event_loop
                .process_ordered_terminal_event(ordered_terminal_event_from_bytes("a", 1), ctx);
        });

        // Ensure the events were applied in the right order.
        let command_grid = model
            .lock()
            .block_list()
            .active_block()
            .command_to_string()
            .trim()
            .to_string();
        assert_eq!(command_grid, "abc");
    })
}

// LOCAL FORK: `command_execution_finished_defers_queued_command_advance_until_block_completion`
// and `command_execution_started_preserves_draft_for_queued_command` both covered the
// agent's queued-query bookkeeping (`QueuedQueryModel` / `BlocklistAIHistoryModel`).
// The viewer no longer special-cases queued agent commands — `CommandExecutionStarted`
// now always clears the input — so there is no behavior left to protect.

#[test]
fn test_pty_bytes_buffered_before_command_execution_started() {
    App::test((), |mut app| async move {
        let channel_event_proxy = ChannelEventListener::new_for_test();
        let model = Arc::new(FairMutex::new(terminal_model_for_viewer(
            channel_event_proxy.clone(),
        )));

        let terminal_view = terminal_view(&mut app);
        let active_block: SerializedBlock = model.lock().block_list().active_block().into();
        let event_loop = app.add_model(|ctx| {
            EventLoop::new(
                model.clone(),
                terminal_view.downgrade(),
                channel_event_proxy.clone(),
                WindowSize {
                    num_rows: 0,
                    num_cols: 0,
                },
                Scrollback {
                    blocks: vec![ScrollbackBlock {
                        raw: serde_json::to_vec(&active_block).unwrap(),
                    }],
                    is_alt_screen_active: false,
                },
                None,
                SharedSessionInitialLoadMode::ReplaceFromSessionScrollback,
                RemoteUpdateGuard::new(),
                ctx,
            )
        });

        // Edge case: PTY bytes arrive BEFORE CommandExecutionStarted.
        // The event loop should buffer the PTY bytes until CommandExecutionStarted arrives,
        // then process them in order.
        event_loop.update(&mut app, |event_loop, ctx| {
            // PTY bytes arrive first (event_no 0-2, out of order)
            event_loop
                .process_ordered_terminal_event(ordered_terminal_event_from_bytes("c", 2), ctx);
            event_loop
                .process_ordered_terminal_event(ordered_terminal_event_from_bytes("a", 0), ctx);

            // CommandExecutionStarted arrives later (event_no 3)
            event_loop.process_ordered_terminal_event(
                OrderedTerminalEvent {
                    event_no: 3,
                    event_type: OrderedTerminalEventType::CommandExecutionStarted {
                        participant_id: Default::default(),
                        ai_metadata: None,
                    },
                },
                ctx,
            );

            // More PTY bytes arrive after CommandExecutionStarted (event_no 4)
            event_loop
                .process_ordered_terminal_event(ordered_terminal_event_from_bytes("b", 1), ctx);
        });

        // Ensure the buffering worked correctly and bytes were applied in the right order.
        // Note: The first two bytes (0, 2) arrive before CommandExecutionStarted,
        // but since the block isn't started until event 3, they should be buffered.
        // After CommandExecutionStarted, the block is started and we process in order: 0, 1, 2.
        let command_grid = model
            .lock()
            .block_list()
            .active_block()
            .command_to_string()
            .trim()
            .to_string();
        assert_eq!(command_grid, "abc");
    })
}

// LOCAL FORK: the `CloudModeSetupPhaseEnded` tests (setup-command group teardown)
// asserted on `TerminalView::ambient_agent_view_model()`, which went out with the
// agent along with the cloud-mode setup phase itself.
