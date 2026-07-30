use std::borrow::Cow;

use anyhow::{Result, anyhow};
use rust_embed::RustEmbed;
use warpui_core::AssetProvider;

#[derive(Clone, Copy, RustEmbed)]
#[folder = "../../app/assets"]
#[include = "bundled/**"] // Should be kept in sync with BUNDLED_ASSETS_DIR.
#[include = "async/**"] // Should be kept in sync with ASYNC_ASSETS_DIR.
#[cfg_attr(target_family = "wasm", exclude = "async/**")]
// Excludes take precedence.
// Standalone CLI builds (the `oz` tarball) are headless and never render the
// onboarding/theme imagery in `async/`, so we exclude those bytes from the
// embedded asset set to keep the CLI binary small — mirroring the carve-out
// already applied for the WASM target above.
#[cfg_attr(feature = "standalone", exclude = "async/**")]
//
// LOCAL FORK: drop the onboarding and product-launch imagery.
//
// rust-embed inlines these bytes into `__const` with no compression, and
// `app/assets/async/png/onboarding` alone is 41MB of the shipped binary. This
// fork boots straight into the terminal — the login wall and the intention
// pickers that display these images are bypassed in `root_view.rs` — so the
// bytes ship without any reachable code path that can show them.
//
// The remaining excludes are launch-modal art for features this fork does not
// have (cloud agents, orchestration, code review, credits/trial upsells).
//
// These are excluded from *embedding* only; the files stay on disk so the asset
// macros still resolve at compile time. The references that remain are runtime
// string lookups from unreachable views, so a miss here is inert.
#[exclude = "async/png/onboarding/**"]
#[exclude = "async/png/Trial-Image.png"]
#[exclude = "async/png/oz_*.png"]
#[exclude = "async/png/agents_3_*.png"]
#[exclude = "async/png/code_launch_*.png"]
#[exclude = "async/png/codex_integration.png"]
#[exclude = "async/png/concurrency_limit_header.png"]
pub struct Assets;

impl AssetProvider for Assets {
    fn get(&self, path: &str) -> Result<Cow<'_, [u8]>> {
        <Assets as RustEmbed>::get(path)
            .map(|f| f.data)
            .ok_or_else(|| anyhow!("no asset exists at path {}", path))
    }
}
