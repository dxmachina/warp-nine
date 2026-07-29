# Warp de-bloat / de-cloud excision manifest

Author: Sebastian Katz

Working notes for stripping login, agents, and cloud from the Warp OSS checkout
(`warp/`, upstream `warpdotdev/warp` @ `3fdb2dec6`), targeting an
Apple-Silicon-only local build.

## Measured baseline (stock `/Applications/Warp.app`, 857 MB)

| Component | Size |
|---|---|
| `Contents/MacOS/stable` (fat binary) | 840 MB |
| — x86_64 slice | 426 MB |
| — arm64 slice | 414 MB |
| `Contents/Frameworks` (Sentry) | 32 MB |
| `Contents/Helpers` (pprof) | 12 MB |
| `Contents/Resources` | 6.5 MB |
| `Contents/PlugIns` (DockTile, fat) | 4.4 MB |

arm64 slice section breakdown:

| Section | Size | Notes |
|---|---|---|
| `__TEXT.__text` | 126 MB | actual code |
| `__TEXT.__const` | 167 MB | anonymous static data |
| `__LINKEDIT` | 90 MB | symbols + line tables (`debug = 1`) |
| `__TEXT.__eh_frame` | 15 MB | |
| `__TEXT.__gcc_except_tab` | 7.8 MB | |
| `__TEXT.__unwind_info` | 2.6 MB | |

Measured `strip -x` on the arm64 slice: **414 MB → 331 MB**.

So config alone (arm64-only + strip) ≈ **857 MB → ~348 MB**, no code changes.

## Key architectural findings

1. **Cargo features are runtime flags, not compile gates.** `agent_mode` appears
   as a `#[cfg(feature = ...)]` gate exactly once; `app/src/features.rs` maps
   cargo features onto `FeatureFlag::set_enabled()` booleans. Disabling features
   hides UI but compiles all the code in. **Only deletion shrinks the binary.**

2. **`app/src/ai/blocklist` is not the terminal block list.** Its docstring:
   *"Implementation of AI blocks used to render AI queries and outputs in the
   blocklist."* The real terminal block model is `app/src/terminal/model/{block,
   blockgrid,blocks}.rs` + `app/src/terminal/block_list_*.rs`. `app/src/ai/` is
   deletable in full.

3. **`skip_login` already exists** (`app/Cargo.toml:847` →
   `warp_server_client/skip_login` → `warp_server_auth/skip_login`). It injects a
   fake test credential and hard-fails all authenticated requests. Useful as an
   interim step, but it does not remove code.

## Deletion manifest

### app/src — agent / AI (~353K LOC)

| Module | LOC |
|---|---|
| `ai` | 261,109 |
| `code` (codebase indexing/embedding) | 38,710 |
| `code_review` | 26,573 |
| `notebooks` | 22,742 |
| `ai_assistant` | 3,637 |

### app/src — cloud / account (~37K LOC)

| Module | LOC |
|---|---|
| `drive` | 22,851 |
| `cloud_object` | 6,986 |
| `auth` | 6,799 |
| `billing` | 492 |

### crates (~61K LOC)

| Crate | LOC | Reason |
|---|---|---|
| `ai` | 30,744 | agent core |
| `computer_use` | 11,834 | agent screen control |
| `cloud_object_models` | 3,909 | cloud sync |
| `warp_server_client` | 3,638 | authed API client |
| `mcp` | 2,691 | agent tool protocol |
| `cloud_objects` | 2,401 | cloud sync |
| `input_classifier` | 2,034 | **embeds 3 ONNX models via `rust-embed`** |
| `warp_server_auth` | 1,491 | login |
| `cloud_object_persistence` | 1,006 | cloud sync |
| `cloud_object_client` | 372 | cloud sync |
| `warp_multi_agent_client` | 240 | agent orchestration |
| `firebase` | 145 | login |

### settings_view pages (~30K of 80K LOC)

| File | LOC |
|---|---|
| `ai_page.rs` | 10,164 |
| `teams_page.rs` | 4,477 |
| `billing_and_usage_page.rs` | 3,605 |
| `code_page.rs` | 3,012 |
| `billing_and_usage_page_v2.rs` | 2,252 |
| `environments_page.rs` | 2,095 |
| `custom_inference_modal.rs` | 1,226 |
| `referrals_page.rs` | 1,125 |
| `agent_assisted_environment_modal.rs` | 767 |
| `mcp_servers_page.rs` | 589 |
| `custom_router_view.rs` | 436 |
| `warp_drive_page.rs` | 282 |
| `set_default_model_modal.rs` | 213 |

### `SettingsSection` enum (`app/src/settings_view/mod.rs:246`)

Keep: `About`, `Appearance`, `Features`, `Keybindings`, `Privacy`, `Scripting`,
`Warpify`.

Remove: `Account` (currently `#[default]` — reassign to `Appearance`),
`MCPServers`, `BillingAndUsage`, `Referrals`, `SharedBlocks`, `Teams`,
`WarpDrive`, `AI`, `WarpAgent`, `AgentProfiles`, `AgentMCPServers`, `Knowledge`,
`ThirdPartyCLIAgents`, `Code`, `CodeIndexing`, `EditorAndCodeReview`,
`CloudEnvironments`, `OzCloudAPIKeys`.

### Also in scope

- `app/src/server` + `crates/graphql` + `crates/warp_graphql_schema` — 48,508 LOC
  of cloud API layer. Needs triage: some is the local control server, not cloud.
- `app/src/crash_reporting` (1,194) + Sentry framework (32 MB in bundle).
- Telemetry/analytics: 400 files mention `telemetry`/`analytics`.
- `app/assets/windows` — 120 MB of Windows binaries (not in the mac bundle, but
  dead weight in the checkout).

## Excision order (dependents before dependencies)

1. Leaf UI: settings pages, menus, command palette, onboarding slides.
2. Terminal/pane/workspace integration points (410 external `ai::` refs).
3. `app/src/ai` + `crates/ai` + agent crates.
4. Cloud: drive, cloud_object, graphql.
5. Auth: `app/src/auth`, `warp_server_auth`, `warp_server_client`, `firebase`.
6. Telemetry + crash reporting.

## Build config changes already applied

- `script/macos/bundle`: `UNIVERSAL_BINARY=false`, `TARGET_ARCH="aarch64"`;
  added `--universal` opt-in flag.
- `Cargo.toml` `[profile.release]`: `debug = 0`, `strip = "symbols"`
  (was `debug = 1`).

## Toolchain

rustup installed with `--no-modify-path`; Homebrew Rust untouched.
Builds require `export PATH="$HOME/.cargo/bin:$PATH"` (pins to 1.92.0 per
`rust-toolchain.toml`). `cargo-bundle` installed from the pinned upstream rev.

## Licensing

Warp is AGPL v3 (except `warpui`/`warpui_core`, MIT). Fine for personal use;
distributing a modified build triggers source-disclosure obligations.
