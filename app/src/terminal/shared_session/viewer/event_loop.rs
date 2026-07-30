use std::collections::HashMap;
use std::io::{Sink, sink};
use std::sync::Arc;

use parking_lot::FairMutex;
use session_sharing_protocol::common::{
    OrderedTerminalEvent, OrderedTerminalEventType, Scrollback, WindowSize,
};
use warpui::{Entity, ModelContext, SingletonEntity, WeakViewHandle};

use crate::terminal::event_listener::ChannelEventListener;
use crate::terminal::model::ansi::{self};
use crate::terminal::model::block::AgentInteractionMetadata;
use crate::terminal::shared_session::shared_handlers::RemoteUpdateGuard;
use crate::terminal::shared_session::{SharedSessionStatus, decode_scrollback};
use crate::terminal::{TerminalModel, TerminalView};

/// If we end up buffering more than this many events,
/// this is an indication that we're too far ahead and
/// could indicate an issue.
const TOO_MANY_BUFFERED_EVENTS: usize = 50;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedSessionInitialLoadMode {
    /// Replace the viewer's placeholder block list with the scrollback snapshot from the session
    /// being joined.
    ReplaceFromSessionScrollback,
    /// Add only the new blocks from a follow-up session while preserving the existing shared
    /// ambient-agent transcript.
    AppendFollowupScrollback,
}

/// The event loop is used to process a stream of events
/// originating from the sender.
pub struct EventLoop {
    terminal_model: Arc<FairMutex<TerminalModel>>,

    /// We need a reference to the view in the event loop
    /// to ensure that any events which require updating the
    /// view and model happen in lockstep. For example,
    /// resize requires updating the view and model.
    /// If we just dispatched an event, we could potentially
    /// have other [`OrderedTerminalEvent`]s race which would
    /// break the invariant of the event loop.
    #[allow(dead_code)]
    terminal_view: WeakViewHandle<TerminalView>,

    parser: ansi::Processor,

    /// We use a sink as a no-op writer to swallow any writes when the ansi handler needs
    /// to write back to the PTY after reading (e.g. to identify itself).
    /// We assume that the sharer will perform these write-backs.
    sink: Sink,

    channel_event_listener: ChannelEventListener,
    remote_update_guard: RemoteUpdateGuard,

    /// The next event number we need from the server.
    next_event_no: usize,

    /// The latest event no of the session the viewer needs to catch up to, at the time of joining.
    catching_up_to_event_no: Option<usize>,

    /// A buffer to maintain events we receive from the server that are unordered.
    buffer: HashMap<usize, OrderedTerminalEventType>,
    // LOCAL FORK: field should_suppress_existing_agent_conversation_replay removed with
    // the agent — only the AI controller ever read it.
}

impl EventLoop {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        terminal_model: Arc<FairMutex<TerminalModel>>,
        terminal_view: WeakViewHandle<TerminalView>,
        channel_event_listener: ChannelEventListener,
        window_size: WindowSize,
        scrollback: Scrollback,
        catching_up_to_event_no: Option<usize>,
        load_mode: SharedSessionInitialLoadMode,
        remote_update_guard: RemoteUpdateGuard,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        let scrollback_blocks = decode_scrollback(&scrollback);
        let is_alt_screen_active = scrollback.is_alt_screen_active;
        {
            let mut terminal_model = terminal_model.lock();
            match load_mode {
                SharedSessionInitialLoadMode::ReplaceFromSessionScrollback => {
                    terminal_model.load_shared_session_scrollback(scrollback_blocks.as_slice());
                }
                SharedSessionInitialLoadMode::AppendFollowupScrollback => {
                    terminal_model
                        .append_followup_shared_session_scrollback(scrollback_blocks.as_slice());
                }
            }
            if is_alt_screen_active {
                terminal_model.enter_alt_screen(true);
            }
        }

        // When we load scrollback, we might not actually complete a block (e.g. shared session started
        // without any scrollback except active block). In this case, we want to make sure the input
        // is aware of what the latest block ID is.
        if let Some(terminal_view) = terminal_view.upgrade(ctx) {
            terminal_view.update(ctx, |terminal_view, ctx| {
                terminal_view.input().update(ctx, |input, ctx| {
                    input.refresh_deferred_remote_operations(ctx);
                });
            });
        }

        if catching_up_to_event_no.is_none() {
            terminal_model
                .lock()
                .set_shared_session_status(SharedSessionStatus::ActiveViewer {
                    role: Default::default(),
                });
        }

        let mut event_loop = Self {
            terminal_model,
            terminal_view,
            parser: ansi::Processor::new(),
            sink: sink(),
            channel_event_listener,
            remote_update_guard,
            // Eventually once we have pagination, the server might need to tell us this.
            next_event_no: 0,
            buffer: HashMap::new(),
            catching_up_to_event_no,
        };

        // Respect the sharer's window size.
        event_loop.process_resize_event(window_size, ctx);

        event_loop
    }

    fn process_resize_event(&mut self, new_window_size: WindowSize, ctx: &mut ModelContext<Self>) {
        if let Some(view) = self.terminal_view.upgrade(ctx) {
            view.update(ctx, |view, ctx| {
                view.resize_from_sharer_update(new_window_size, ctx);
            });
        }
    }

    /// Returns None if we haven't received any events yet.
    pub fn last_received_event_no(&self) -> Option<usize> {
        if self.next_event_no == 0 {
            return None;
        }
        Some(self.next_event_no - 1)
    }

    pub fn process_ordered_terminal_event(
        &mut self,
        event: OrderedTerminalEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        // Add the event to the buffer.
        self.buffer.insert(event.event_no, event.event_type);

        // If we get too far ahead, let's log a warning for better debugging.
        if self.buffer.len() >= TOO_MANY_BUFFERED_EVENTS {
            log::warn!(
                "Viewer is more than {TOO_MANY_BUFFERED_EVENTS} events ahead of next_event_no"
            );
        }

        // Flush out as many contiguous events as we can.
        while let Some(next_event) = self.buffer.remove(&self.next_event_no) {
            let _active_remote_update = self.remote_update_guard.start_remote_update();
            match next_event {
                OrderedTerminalEventType::PtyBytesRead { bytes } => {
                    let mut model = self.terminal_model.lock();
                    let decompressed = lz4_flex::block::decompress_size_prepended(&bytes)
                        .expect("Should be able to decompress the PtyBytesRead event");
                    self.parser
                        .parse_bytes(&mut *model, &decompressed, &mut self.sink);
                }
                OrderedTerminalEventType::CommandExecutionStarted {
                    participant_id,
                    ai_metadata,
                } => {
                    let should_clear_input = ai_metadata.is_none();

                    // LOCAL FORK: the sharer's AI metadata was mapped back to a
                    // conversation through the agent's history model. With the
                    // agent gone there is nothing to reconstruct.
                    let reconstructed_ai_metadata: Option<AgentInteractionMetadata> = None;

                    let start_outcome = self
                        .terminal_model
                        .lock()
                        .start_command_execution_for_shared_session(
                            participant_id,
                            reconstructed_ai_metadata.clone(),
                        );

                    // Notify the action model that the action is now executing on the sharer's side
                    // This allows the viewer's UI to show the command as running rather than queued
                    // (which is essential for long running commands to be expandable in the UI).
                    if start_outcome.is_accepted() {
                        // When a non-agent command starts, clear the loading state and input buffer.
                        // We don't clear for agent commands because the viewer may be typing a
                        // follow-up.
                        if should_clear_input && let Some(view) = self.terminal_view.upgrade(ctx) {
                            view.update(ctx, |view, ctx| {
                                // LOCAL FORK: this used to skip the clear during cloud-agent
                                // setup so a follow-up the viewer was composing survived. Both
                                // the setup phase and the queued-command check went with the
                                // agent.
                                view.input().update(ctx, |input, ctx| {
                                    // Restore frozen visual state. Then also reinitialize the
                                    // buffer here: for shell commands the block transition will
                                    // reset the CRDT with a new block ID shortly after, so the
                                    // brief CRDT inconsistency is harmless. This pre-emptive
                                    // clear gives the viewer an empty buffer while the command
                                    // runs rather than showing the command text.
                                    input.unfreeze_agent_input(false, ctx);
                                    let editor = input.editor().clone();
                                    editor.update(ctx, |editor, ctx| {
                                        editor.reinitialize_buffer(None, ctx);
                                    });
                                });
                            });
                        }
                        // LOCAL FORK: remote-execution marking went with the agent.
                    }
                }
                OrderedTerminalEventType::Resize { window_size } => {
                    self.process_resize_event(window_size, ctx)
                }
                OrderedTerminalEventType::CommandExecutionFinished { .. } => {
                    // Queue advancement waits for block completion so input cleanup can observe
                    // the in-flight queued command and preserve any local draft.
                }
                // LOCAL FORK: the sharer's agent responses were replayed into the viewer's
                // AI controller, which went with the agent. The protocol still delivers
                // these events, so the arms stay and drop them.
                OrderedTerminalEventType::AgentResponseEvent { .. } => {}
                OrderedTerminalEventType::AgentConversationReplayStarted => {
                    self.terminal_model
                        .lock()
                        .set_is_receiving_agent_conversation_replay(true);
                }
                OrderedTerminalEventType::AgentConversationReplayEnded => {
                    self.terminal_model
                        .lock()
                        .set_is_receiving_agent_conversation_replay(false);
                }
                // LOCAL FORK: the cloud-mode setup phase and its queued-prompt drain went
                // with the agent.
                OrderedTerminalEventType::CloudModeSetupPhaseEnded => {}
            }

            if Some(self.next_event_no) == self.catching_up_to_event_no
                && let Some(view) = self.terminal_view.upgrade(ctx)
            {
                // TODO (suraj): reconsider how we query the role here.
                if let Some(presence_manager) =
                    view.read(ctx, |view, _| view.shared_session_presence_manager())
                {
                    // Role is set to the presence manager's role to stay as up-to-date as possible.
                    // This avoids a race condition if a viewer gets a new role before catching up,
                    // by ensuring we're not overwriting the new role.
                    if let Some(role) = presence_manager.as_ref(ctx).role() {
                        self.terminal_model
                            .lock()
                            .set_shared_session_status(SharedSessionStatus::ActiveViewer { role });
                    }
                }
            }

            self.channel_event_listener.send_wakeup_event();

            self.next_event_no += 1;
        }
    }
}

impl Entity for EventLoop {
    type Event = ();
}

#[cfg(test)]
#[path = "event_loop_tests.rs"]
mod tests;
