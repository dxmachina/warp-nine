//! Regression tests for the viewer `TerminalManager`'s session-end handling.
//!
//! LOCAL FORK: this file used to be mostly about the orchestration viewer model
//! (OVM) — `on_view_detached` tearing it down on `DetachType::Closed` while
//! preserving it on `HiddenForClose` (undo-close grace window) and `Moved`, plus
//! the agent's queued-command-in-flight bookkeeping. The OVM,
//! `OrchestrationEventStreamer`, `BlocklistAIHistoryModel` and `QueuedQueryModel`
//! all went out with the agent, so those tests went with them. The stale
//! session-end guard below covers the shared-session viewer, which this fork
//! keeps.

use async_broadcast::broadcast;
use warpui::App;

use super::*;
use crate::test_util::add_window_with_terminal;
use crate::test_util::terminal::initialize_app_for_terminal_view;

#[test]
fn handle_viewer_session_end_ignores_stale_ambient_end() {
    // A stale ambient end (the ended network is no longer the current one) must
    // be ignored: `handle_viewer_session_end` routes ambient panes through
    // `end_current_ambient_session`, whose current-network guard bails, so the
    // helper returns `false` and performs no teardown.
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal_view = add_window_with_terminal(&mut app, None);
        let model = Arc::new(FairMutex::new(TerminalModel::mock(None, None)));

        let (wakeups_tx, _wakeups_rx) = async_channel::unbounded();
        let (events_tx, _events_rx) = async_channel::unbounded();
        let (pty_reads_tx, pty_reads_rx) = broadcast(8);
        let _inactive_pty_reads_rx = pty_reads_rx.deactivate();
        let channel_event_proxy = ChannelEventListener::new(wakeups_tx, events_tx, pty_reads_tx);
        let (_write_to_pty_tx, write_to_pty_rx) = async_channel::unbounded();

        let ended_network = app.add_model(|ctx| {
            Network::new_for_test(
                channel_event_proxy,
                terminal_view.downgrade(),
                model.clone(),
                write_to_pty_rx,
                RemoteUpdateGuard::new(),
                ctx,
            )
        });

        // Empty `current_network` => the ended network is stale.
        let current_network = Arc::new(FairMutex::new(None));

        let mut handled = true;
        app.update(|ctx| {
            handled = TerminalManager::handle_viewer_session_end(
                &terminal_view,
                model.clone(),
                &current_network,
                &ended_network,
                /* is_ambient_agent */ true,
                ctx,
            );
        });

        assert!(
            !handled,
            "a stale ambient end (ended network != current) must be ignored"
        );
        assert!(
            !model.lock().shared_session_status().is_finished_viewer(),
            "an ignored stale ambient end must not finish the viewer"
        );
    });
}
