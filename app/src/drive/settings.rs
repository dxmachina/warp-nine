// LOCAL FORK: two of this group's three settings went with the Warp Drive browser --
// `sorting_choice` (the index's sort menu, TOML `warp_drive.sorting_choice`) and
// `enable_warp_drive` (the tools-panel tab toggle, TOML `warp_drive.enabled`), along with
// `is_warp_drive_enabled`/`is_warp_drive_available`. `sharing_onboarding_block_shown` is not
// about Drive: `terminal/view.rs` sets it when it inserts the session-sharing onboarding block
// and `workspace/view.rs` reads it to decide whether to insert one. Session sharing is kept, so
// the group survives for that one field.
use settings::macros::define_settings_group;
use settings::{RespectUserSyncSetting, SupportedPlatforms, SyncToCloud};

define_settings_group!(WarpDriveSettings, settings: [
    sharing_onboarding_block_shown: WarpDriveSharingOnboardingBlockShown {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        surface: settings::SettingSurfaces::GUI,
        private: true,
    },
]);
