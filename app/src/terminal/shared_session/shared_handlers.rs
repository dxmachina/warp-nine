use std::cell::Cell;
use std::rc::Rc;

use input_classifier::InputType;
use session_sharing_protocol::common::{
    CLIAgentSessionState, InputMode, InputType as ProtocolInputType, SelectedAgentModel,
    SelectedConversation, ServerConversationToken, UniversalDeveloperInputContextUpdate,
};
use warp_core::features::FeatureFlag;
use warpui::{AppContext, ModelHandle, SingletonEntity, WeakViewHandle};



/// Handles updating the local input mode when an input mode update is received.
/// This function is shared between the viewer and sharer to ensure consistent behavior.
pub(crate) fn apply_input_mode_update(
    weak_view_handle: &WeakViewHandle<TerminalView>,
    input_mode: &InputMode,
    _guard: &ActiveRemoteUpdate,
    ctx: &mut AppContext,
) {
    let Some(view) = weak_view_handle.upgrade(ctx) else {
        return;
    };

    // When AgentView is enabled, we only apply input mode updates when in an active agent view.
    // Outside of agent view, input mode changes are not relevant.
    if FeatureFlag::AgentView.is_enabled() {
        let agent_view_controller = view.as_ref(ctx).agent_view_controller().clone();
        if !agent_view_controller.as_ref(ctx).is_active() {
            return;
        }
    }

    let client_input_type = match input_mode.input_type {
        ProtocolInputType::Shell => InputType::Shell,
        ProtocolInputType::AI => InputType::AI,
    };
    let new_config = InputConfig {
        is_locked: input_mode.is_locked,
    };

    // Skip update if nothing would change
    let current_config = view.as_ref(ctx).input_config(ctx);
    if current_config == new_config {
        return;
    }

    view.update(ctx, |terminal_view, ctx| {
        terminal_view.apply_external_input_mode_update(new_config, ctx);
    });
}

/// Handles updating the local auto-approve setting when an update is received.
/// This function is shared between the viewer and sharer to ensure consistent behavior.
pub(crate) fn apply_auto_approve_agent_actions_update(
    weak_view_handle: &WeakViewHandle<TerminalView>,
    auto_approve: bool,
    _guard: &ActiveRemoteUpdate,
    ctx: &mut AppContext,
) {
    let Some(view) = weak_view_handle.upgrade(ctx) else {
        return;
    };

    view.update(ctx, |view, ctx| {
        let ai_context_model = view.ai_context_model().clone();
        ai_context_model.update(ctx, |context_model, ctx| {
            let current_mode = context_model.pending_query_autoexecute_override(ctx);
            let is_on = current_mode.is_autoexecute_any_action();

            // Skip if we're already in the desired state to avoid feedback loops.
            if is_on == auto_approve {
                return;
            }

            context_model.toggle_pending_query_autoexecute(ctx);
        });
    });
}






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
