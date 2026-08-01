use serde::Deserialize;
use warp_core::user_preferences::GetUserPreferences as _;
use warpui::{Entity, ModelContext};

// Note: The name of this key is from before this model was created. For consistency, it should
// remain the same value
const SENT_REFERRAL_THEME_KEY: &str = "ReferralThemeActive";
const RECEIVED_REFERRAL_THEME_KEY: &str = "ReceivedReferralTheme";

/// Model to track the status of referral theme(s)
///
/// Note: An invariant of this type, relied upon by the rest of the code, is that themes will only
/// ever become available, they can not be revoked.
///
/// LOCAL FORK: this is now a read-only view of what is already stored locally.
///
/// The fetch half -- `query_referral_status`, its response handler and the
/// `ReferralsClient` behind them -- is gone. It was already unreachable: the
/// query returned early unless the user was logged in, and login was removed.
///
/// The model itself is deliberately kept. It reads `ReferralThemeActive` and
/// `ReceivedReferralTheme` out of user preferences, which are persisted, so
/// someone who earned a referral theme before this fork existed still has it in
/// their settings and `theme_chooser` still offers it to them. Deleting the model
/// and hard-coding those two themes as hidden would have quietly taken away a
/// theme people had already unlocked. Nothing can move a theme from locked to
/// unlocked any more, which is the only behaviour actually lost here.
pub struct ReferralThemeStatus {
    sent_referral_theme: ReferralThemeFetchStatus,
    received_referral_theme: ReferralThemeFetchStatus,
}

impl Entity for ReferralThemeStatus {
    // LOCAL FORK: was `ReferralThemeEvent`, emitted only when a server response
    // moved a theme from locked to unlocked. With no server there is no such
    // transition, so nothing is emitted and the event type has no variants left
    // to describe.
    type Event = ();
}

impl ReferralThemeStatus {
    /// Creates a new ReferralThemeStatus model
    ///
    /// The initial values for the theme availability will be loaded from user default storage
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let sent_referral_theme = parse_sent_referral_fetch_status(
            ctx.private_user_preferences()
                .read_value(SENT_REFERRAL_THEME_KEY)
                .unwrap_or_default(),
        );

        let received_referral_theme = ctx
            .private_user_preferences()
            .read_value(RECEIVED_REFERRAL_THEME_KEY)
            .unwrap_or_default()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(ReferralThemeFetchStatus::NotFetched);

        Self {
            sent_referral_theme,
            received_referral_theme,
        }
    }

    /// Is the "Sent Referral" theme available (i.e. has the user sent at least one referral)?
    pub fn sent_referral_theme_active(&self) -> bool {
        self.sent_referral_theme.is_active()
    }

    /// Is the "Received Referral" theme available (i.e. was the user referred by another)?
    pub fn received_referral_theme_active(&self) -> bool {
        self.received_referral_theme.is_active()
    }
}

/// Type used for tracking the fetch status of different referral themes
///
/// For the received referral theme, we only need to check until we get a successful response.
/// Since the user can only sign up once and they were either referred or not, the response should
/// be definitive.
///
/// For the sent referral theme, we still need to keep checking even if a previous response
/// indicated that it wasn't available, since the user could have sent a referral in the interim.
#[derive(Deserialize, Clone, Copy)]
enum ReferralThemeFetchStatus {
    NotFetched,
    Inactive,
    Active,
}

impl ReferralThemeFetchStatus {
    fn is_active(self) -> bool {
        matches!(self, ReferralThemeFetchStatus::Active)
    }
}

/// Parse the sent referral status into a ReferralThemeFetchStatus
///
/// Note: For historical reasons, the status is stored as a boolean literal (`true` or `false`),
/// so we need to map that onto the fetch status.
fn parse_sent_referral_fetch_status(stored_value: Option<String>) -> ReferralThemeFetchStatus {
    stored_value
        .and_then(|s| s.parse::<bool>().ok())
        .map(|active| {
            if active {
                ReferralThemeFetchStatus::Active
            } else {
                ReferralThemeFetchStatus::Inactive
            }
        })
        .unwrap_or(ReferralThemeFetchStatus::NotFetched)
}
