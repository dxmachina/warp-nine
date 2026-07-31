use settings::Setting as _;
use warpui::{App, SingletonEntity};

use super::{PrivacySettings, WarpDrivePrivacySettings, seed_default_secret_regexes};
use crate::auth::AuthStateProvider;
use crate::server::server_api::ServerApiProvider;
use crate::settings::manager::SettingsManager;

/// Registering `PrivacySettings` must leave the terminal with a non-empty set of secret
/// patterns.
///
/// This is a regression test for a fork-specific bug, not an upstream behaviour. The
/// default patterns are seeded by `initialize_default_regexes_once`, whose only upstream
/// caller runs after `UpdateManager::initial_load_complete()` resolves. That future needs a
/// cloud object fetch, which needs an account, so with login removed it never resolved and
/// a fresh install ended up with an empty `user_secret_regex_list`. Since that list is the
/// sole input to `set_user_and_enterprise_secret_regexes`, terminal obfuscation silently
/// matched nothing.
///
/// The failure mode is quiet in two ways worth guarding against: it only affects fresh
/// databases, because the list is persisted, and an empty pattern set looks exactly like a
/// working one until you check whether a secret was actually redacted.
#[test]
fn default_secret_regexes_are_seeded_without_a_cloud_load() {
    App::test((), |mut app| async move {
        app.update(crate::settings::init_and_register_user_preferences);
        app.add_singleton_model(|_| SettingsManager::default());
        app.add_singleton_model(|_| ServerApiProvider::new_for_test());
        app.add_singleton_model(|_| AuthStateProvider::new_for_test());
        WarpDrivePrivacySettings::register(&mut app);

        app.update(PrivacySettings::register_singleton);
        // The startup seed, standing in for the call `lib.rs` makes. No cloud object fetch
        // is performed anywhere in this test.
        app.update(seed_default_secret_regexes);

        app.read(|ctx| {
            let privacy_settings = PrivacySettings::as_ref(ctx);
            assert!(
                !privacy_settings.user_secret_regex_list.is_empty(),
                "registering PrivacySettings should seed the default secret patterns; \
                 an empty list means terminal obfuscation has nothing to match"
            );
            assert!(
                *privacy_settings
                    .has_initialized_default_secret_regexes
                    .value(),
                "the one-time seed flag should be set so the defaults are not re-added"
            );
        });
    });
}
