# WarpNine

A personal fork of [Warp](https://github.com/warpdotdev/warp) with the account
system, coding agent, and cloud backend removed. 857 MB to 78 MB. No sign-in, no
telemetry.

Not affiliated with or endorsed by Warp Dev, Inc.

## Download

| | For | Size | Minimum macOS |
|---|---|---|---|
| **[WarpNine-arm64.dmg](https://github.com/dxmachina/warp-nine/releases/latest/download/WarpNine-arm64.dmg)** | Apple Silicon (M1 and later) | 31 MB | 11.0 Big Sur |
| **[WarpNine-x86_64.dmg](https://github.com/dxmachina/warp-nine/releases/latest/download/WarpNine-x86_64.dmg)** | Intel | 34 MB | 10.14 Mojave |

Which one: Apple menu → About This Mac. "Apple M1" or later is arm64.

Open the image, drag WarpNine to Applications, launch. Developer ID signed and
notarized by Apple, so it opens on a double click — no Gatekeeper prompt.

```bash
shasum -a 256 ~/Downloads/WarpNine-arm64.dmg
spctl -a -vvv -t exec /Volumes/WarpNine/WarpNine.app
# source=Notarized Developer ID
```

Those links always resolve to the newest build; per-build checksums are on the
[release page](https://github.com/dxmachina/warp-nine/releases/latest).

## What this is

Warp's terminal: blocks and the block list, themes, Warpify shell integration,
local completions, workflows, keybindings, settings. Without the product around it.

## What this isn't

- **Not supported.** Signed so it opens cleanly, not because anyone stands behind it.
- **Not upstream-compatible.** Whole subsystems are deleted; rebasing gets harder
  over time. That's the trade.
- **Not a smaller Warp with the features intact.** Sign-in, the agent, Warp Drive,
  shared sessions, settings sync, notifications and onboarding are gone, not hidden.
- **Not cross-platform.** macOS only. Both architectures build, but only arm64 is
  run daily; Intel is verified as far as a notarized launch under Rosetta. Linux
  and Windows paths are untouched and unexercised.
- **Not audited.** Telemetry and crash reporting are hard-off and verified in the
  logs, but this is one person's fork.

## Current state

| | Stock Warp | WarpNine |
|---|---|---|
| App bundle | 857 MB | **78 MB** arm64 / 84 MB Intel |
| Disk image | n/a | 31 MB arm64 / 34 MB Intel |
| Architectures | one fat binary | two single-arch builds |
| `__text` (code) | 126 MB | 33 MB |
| `__const` (data) | 167 MB | 41 MB |
| Unwinding tables | 25 MB | 0.2 MB |
| `__LINKEDIT` (symbols) | ~75 MB | 0.4 MB |
| Login wall | yes | no |
| Telemetry | yes | no |
| Dock tile plugin | 4.4 MB, universal | removed |

Launches to a shell. `--version` reports a real build stamp.

Measured at `4e8698693`; the lever table below at `3cb1ec862`. Everything excised
between them is worth ~2 MB, because deleting call-path code removes lines, not
bytes. The binary is dominated by dependencies that stay.

## Where the size was

The agent is not why Warp is big. Attributing stock-binary symbol sizes to their
source modules puts the whole agent tree (`warp::ai` plus the `ai`, `rmcp`,
`candle`, `tantivy`, `computer_use`, `input_classifier`, `tokenizers` crates) at
roughly 22 MB of a 395 MB arm64 slice.

What actually cost the megabytes:

| Lever | Saved | Change |
|---|---|---|
| No fat binary | 426 MB | one default flipped; each image carries one slice |
| `debug = 0` + `strip = "symbols"` | ~90 MB | upstream keeps line tables for Sentry |
| Dropped 34 tree-sitter grammars | 51 MB | `arborium` features |
| Excluded onboarding imagery | 47 MB | `rust-embed` inlined 57 MB of PNGs uncompressed |
| `opt-level = "s"` | 26 MB | release defaults to `3`, which optimizes for speed |
| `panic = "abort"` | 25 MB | removes `__eh_frame` / `__gcc_except_tab` |
| Dropped the ONNX classifier | 17.5 MB | `bert_tiny_v3.onnx`, embedded to route shell-vs-agent input |
| Dropped 663 PowerShell specs | 5 MB | dead weight on macOS |

Two are worth knowing about. **41 MB was onboarding screenshots** —
`crates/warp_assets` embeds `app/assets/async` verbatim, uncompressed; the WASM and
headless-CLI builds already excluded it and nobody had for the GUI. **The grammars
weren't for the terminal** — all 38 `arborium` languages served the code editor and
the agent's codebase indexer, while terminal input highlighting lives in
`terminal/input/decorations.rs` and uses the completer's shell tokenizer.

Warp's cargo features are runtime flags, not compile gates: `app/src/features.rs`
maps them onto `FeatureFlag::set_enabled()` booleans, and `agent_mode` appears as a
`#[cfg(feature = ...)]` gate exactly once in the tree. Turning one off usually hides
UI while still compiling it in — which is why an earlier version of this README
concluded "only deletion makes this smaller." The table refutes it: build-profile
and asset changes account for most of the reduction and delete no application code.
Deletion is still the only thing that removes the agent, just not the megabytes.

## What's removed

| | LOC | |
|---|---|---|
| The agent | ~220K | `app/src/ai` and the `ai`, `mcp`, `computer_use`, `input_classifier` crates, plus its settings pages, menu entries, toasts, account menu and notification inbox |
| The cloud object write path | ~20.6K | the sync queue, live GraphQL mutations, thirteen `UpdateManager` methods |
| Shared sessions | ~19.1K | the collaborative terminal: `wss://sessions.app.warp.dev`, Reader / Executor / Full roles, CRDT-shared input, the `warp://shared_session/{id}` URI host |
| The onboarding crate | ~16.9K | guided tutorial, callout view, keybinding builder, HOA flow, Oz and OpenWarp launch modals |
| Warp Essentials | 3.4K | `app/src/resource_center`: tips, changelog feed, Docs/Slack/Feedback links |
| Sign-in | | the login wall, and the startup Keychain read that raised an OS prompt on every freshly signed build |
| Telemetry and crash reporting | | hard-off in `settings/privacy.rs`; upstream's force flag and `AgentModeAnalytics` override are ignored |
| The dock tile plugin | | 4.4 MB, shipped universal regardless of target arch, wrote `/tmp/warp_docktile_<timestamp>.log` on every init |
| Referral and changelog menu items | | one rendered as `<NO DESCRIPTION>`, its label lookup having stopped resolving |

Much of it was already unreachable. Five Warp Drive affordances — Trash, Untrash,
permanent delete, the notebook edit baton and drive sharing — sat behind a
`let Some(server_id) = ... else { return; }` that a locally created object can never
satisfy; the buttons stayed enabled and did nothing. Onboarding was dead three ways
over while still being a default feature. Sharing a session needed a sign-in that no
longer existed, though *joining* someone else's did not.

## What's kept

Terminal and blocks, themes, Warpify, local completions (496 command specs),
workflows, keybindings, settings, the code editor (unhighlighted, see grammars
above), secret detection in terminal output.

## What's left

**`remote_server`** (~20.6K LOC) is the last large subsystem talking to Warp's
infrastructure: it installs and drives a Warp-built helper on an SSH host so blocks
and Warpify work remotely. Removing it degrades SSH to the wrapper-only
warpification path, which stays. A removal is parked as an uncommitted patch —
deletions done, 74 unresolved-module errors across 24 files left. All `E0432`/`E0433`,
so mechanical in kind, but each site needs a decision about the code that used the
import, not just the import.

**Notebooks are not separable.** `app/src/notebooks/editor` is 14,340 of the
subsystem's 21,638 lines and is not notebook-specific — it is the app's rich-text
editor, imported across `code_review/` and `code/editor/`. Removing notebooks whole
takes in-app git diff review with it. Either the product surface goes and the editor
is re-homed under a truthful name, or code review goes too; that's a product
decision.

**`warp_server_client`** (3,022 lines) and **`warp_server_auth`** (1,391) stay. Named
as part of the cloud-object excision but not gated on it: they back `remote_server`,
API key management, and crash reporting.

See [`EXCISION_MANIFEST.md`](EXCISION_MANIFEST.md) for what each pass removed and
what it broke on the way. `script/fork_separability` predicts removal cascades —
distrust its name-frequency signal, which once predicted a 24-file cascade for a
type with zero external users.

## Building

Needs the pinned toolchain from `rust-toolchain.toml` (1.92.0); Homebrew's Rust
ignores the pin.

```bash
rustup toolchain install 1.92.0
cargo install cargo-bundle --git=https://github.com/burtonageo/cargo-bundle \
  --rev ae4c76e92c08774bf54ff077b1c52e3d1cd6c16d
cargo install --locked cargo-about@0.8.4

export PATH="$HOME/.cargo/bin:$PATH"
./script/macos/bundle --channel oss
# -> target/aarch64-apple-darwin/release-lto/bundle/osx/WarpNine.app
```

For a signed, notarized release use `script/release`. It checks both credentials
*before* the build, then mounts the finished image, copies the app out, applies the
quarantine attribute a browser would set, and confirms the extracted copy validates.

```bash
./script/release                 # arm64
./script/release --arch x86_64   # Intel, cross-built; smoke test needs Rosetta
./script/release --arch both     # both, each fully verified before the next starts
./script/release --signed-only   # skip notarization
```

Images land in `target/release-lto/bundle/osx` named for their architecture. That
directory is not per-target, so the loose `.app` beside them is whichever built
last; each image's own app stays under `target/<rust-target>/release-lto/bundle/osx/`.

Verify a local build:

```bash
BUNDLE=target/aarch64-apple-darwin/release-lto/bundle/osx/WarpNine.app
codesign --verify --strict "$BUNDLE"
lipo -archs "$BUNDLE/Contents/MacOS/warp-nine"   # arm64
```

**Minimum macOS: 11.0 arm64, 10.14 Intel.** `.cargo/config.toml` sets
`MACOSX_DEPLOYMENT_TARGET = "10.14"`, which `crates/warpui/build.rs` also pins the
Metal AIR bytecode to. It cannot bind on arm64 — Rust's floor for Apple Silicon is
11.0, since Big Sur is where Apple Silicon started. Nothing sets
`LSMinimumSystemVersion`, so the Mach-O load command is the only gate, and neither
floor has been tested below the machine that builds them. (The login item needs
macOS 13+, and says so in its own label.)

Three things that have cost time here:

- **`cargo-about` is not optional.** It generates `THIRD_PARTY_LICENSES.txt`, a step
  that runs *before* signing. Missing, the script aborted there and never signed,
  leaving the linker's ad-hoc signature over unsealed resources — which macOS reads
  as corrupt rather than unsigned. `prepare_bundled_resources` now warns and continues.
- **Builds are slow.** `lto = "fat"` with `codegen-units = 1` optimizes the whole
  program as one largely single-threaded unit. Tens of minutes is normal; use
  `lto = "thin"` in `[profile.release-lto]` to iterate.
- **`target/` grows without bound.** Cargo never collects superseded artifacts, so
  every rebuild of the `warp` lib crate leaves a ~600 MB `.rlib`. Use `cargo-sweep`.
  Upstream also generated the settings schema without `--target`, duplicating every
  dependency into a second tree; the bundle script now passes the triple through.

`./script/bootstrap` works but installs tooling this fork doesn't need (Docker,
gcloud, PowerShell, Sentry CLI).

## Layout

`main` is the fork; `master` mirrors upstream for rebasing.

```bash
git remote add upstream https://github.com/warpdotdev/warp.git
git fetch upstream && git rebase upstream/master
```

## License

Unchanged from upstream: [AGPL v3](LICENSE-AGPL), except `warpui` and `warpui_core`
under [MIT](LICENSE-MIT). Upstream's copyright notices are preserved in the bundle
metadata and About page, with this fork credited alongside.

`crates/warp_command_signatures` and `crates/warp_completion_metadata` are vendored
from [warpdotdev/command-signatures](https://github.com/warpdotdev/command-signatures)
at `29cd61c3` with the PowerShell specs removed — `rust-embed`'s `folder` and
`exclude` attributes are compile-time literals, so a subtree can't be excluded from
outside the crate.
