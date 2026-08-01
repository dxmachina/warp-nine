//! LOCAL FORK: what is left of `app/src/auth` is the account *state*, not the account.
//!
//! The login flow and everything that drove it went: `AuthManager` and its event enum,
//! the sign-in view and its body, the SSO-link view, the override-warning modal, the
//! web-handoff view, the login error modal and failure notification, the pasted-token
//! path, and the anonymous-user signup machinery. Roughly 4,200 lines across 13 files.
//!
//! `AuthState` itself stays. It lives in `warp_server_auth` and is read by features this
//! build keeps: session sharing, the cloud object model, the remote-server SSH context and
//! crash reporting all ask it for a user id or a logged-in flag. It is now permanently
//! logged out (see `AuthState::initialize`, which this fork pins to `None`), so every one
//! of those reads takes its logged-out branch. Removing the type would mean rewriting
//! those callers; leaving it costs nothing and keeps them on a path upstream also has.
//!
//! Logging out went with logging in. `maybe_log_out`, `log_out_and_open_web` and `log_out`
//! existed to swap accounts: they cleared the sqlite database, reset the cloud object
//! model, stopped the sync and polling loops, dropped cloud-persisted settings and left
//! every shared session. With no way to log in there is no second account to swap to, so
//! the entry points are gone rather than left as buttons that wipe local state.

// LOCAL FORK: `credentials` and `user` are re-exported for `#[cfg(test)]` code only, so
// the lib target reports them unused. Removing them breaks the test build; this is what
// `cargo fix` did twice.
#[allow(unused_imports)]
pub use warp_server_auth::{auth_state, credentials, user, user_uid};

pub use auth_state::AuthStateProvider;
pub use user_uid::UserUid;
