use std::cell::Cell;
use std::rc::Rc;

// LOCAL FORK: fn apply_input_mode_update removed with the agent.
// LOCAL FORK: fn apply_auto_approve_agent_actions_update removed with the agent.

// ---------------------------------------------------------------------------
// Echo-suppression for remote session-sharing context updates.
//
// When a participant (viewer or sharer) receives a
// `UniversalDeveloperInputContextUpdate` from the remote side, the `apply_*`
// helpers above update local state which fires model events. Those events are
// observed by broadcast subscribers that would normally send the value *back*
// over the network, creating an echo loop.
//
// To prevent this, each side creates a `RemoteUpdateGuard` and:
//   1. Clones it into every broadcast subscriber, which calls
//      `guard.should_broadcast()` and skips when `false`.
//   2. Wraps incoming `apply_*` calls with `guard.start_remote_update()`,
//      which returns an `ActiveRemoteUpdate` RAII token that suppresses
//      broadcasts for the duration of the synchronous update.
//
// When adding a **new** field to `UniversalDeveloperInputContextUpdate`:
//   - Check `guard.should_broadcast()` in the new broadcast subscriber.
//   - Ensure the new `apply_*` call sits inside the existing
//     `ActiveRemoteUpdate` scope in the incoming handler.
// ---------------------------------------------------------------------------

/// Shared guard that tracks whether we are currently applying a remote
/// session-sharing context update.
#[derive(Clone)]
pub(crate) struct RemoteUpdateGuard {
    inner: Rc<Cell<bool>>,
}

impl RemoteUpdateGuard {
    /// Creates a new guard, initially not suppressing broadcasts.
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(Cell::new(false)),
        }
    }

    /// Returns `true` when a context update originated locally and should be
    /// broadcast to the remote side. Returns `false` when we are in the middle
    /// of applying a remote update (i.e. the echo should be suppressed).
    pub(crate) fn should_broadcast(&self) -> bool {
        !self.inner.get()
    }

    /// Returns an RAII token that suppresses outgoing broadcasts until dropped.
    /// Wrap all `apply_*` calls for incoming remote updates in this so that
    /// the synchronous event dispatch sees the guard as active.
    pub(crate) fn start_remote_update(&self) -> ActiveRemoteUpdate {
        debug_assert!(
            !self.inner.get(),
            "RemoteUpdateGuard::start_remote_update called while already active"
        );
        self.inner.set(true);
        ActiveRemoteUpdate {
            inner: self.inner.clone(),
        }
    }
}

/// RAII token that suppresses outgoing broadcasts while held.
pub(crate) struct ActiveRemoteUpdate {
    inner: Rc<Cell<bool>>,
}

impl Drop for ActiveRemoteUpdate {
    fn drop(&mut self) {
        self.inner.set(false);
    }
}
