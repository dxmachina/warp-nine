// LOCAL FORK: the error-classification helpers (is_transient_http_error,
// is_transient_graphql_or_http_error, is_transient_status, is_auth_error) and the
// bounded-retry driver (with_bounded_retry plus its MAX_ATTEMPTS/INITIAL_BACKOFF/
// BACKOFF_FACTOR/BACKOFF_JITTER constants) were only ever called from the agent and
// its harness upload path. The four retry strategies below are still used by the
// cloud-object and workspace update managers and the cloud-object listener.

use std::time::Duration;

use warpui::RetryOption;

/// Common duration for a periodic poll. In our app, we generally have the following to update the same data:
/// - RTC messages
/// - Out-of-band queries based on user actions (i.e. fetch team info when user opens the settings page, user
/// starts the app)
/// However, we also periodically poll for updates in case RTC is down, the user's websocket
/// is borked, etc.
/// For team memberships, we also don't yet process messages for joining or leaving a team, so the user would see these
/// updates only after a periodic poll.
pub const PERIODIC_POLL: Duration = Duration::from_secs(60 * 10);

/// For a periodic poll, it's fine to wait for longer period of time between retries. However, we don't want this to be so
/// long that it's around the same as the overall periodic poll interval.
pub const PERIODIC_POLL_RETRY_STRATEGY: RetryOption = RetryOption::exponential(
    Duration::from_secs(2), /* interval */
    2.,                     /* exponential factor */
    3,                      /* max retry count */
)
.with_jitter(0.2 /* max_jitter_percentage */);

/// When there's an out-of-band request for a periodic poll, we want to retry quickly, because the UI is depending on the
/// request succeeding in a timely way. These are things like loading all object updates upon startup, checking the team
/// metadata when we visit the team page, etc.
pub const OUT_OF_BAND_REQUEST_RETRY_STRATEGY: RetryOption = RetryOption::exponential(
    Duration::from_millis(100), /* interval */
    5.,                         /* exponential factor */
    3,                          /* max retry count */
)
.with_jitter(0.5 /* max_jitter_percentage */);

// For listeners, retry up to 5 times, waiting between 10-40 seconds between retries.
pub const LISTENER_RETRY_STRATEGY: RetryOption = RetryOption::linear(
    Duration::from_secs(25), /* interval */
    5,                       /* max retry count */
)
.with_jitter(0.6 /* max_jitter_multiplier */);
