# WarpNine

A personal fork of [Warp](https://github.com/warpdotdev/warp) with the account
system, coding agent, and cloud backend removed.

857 MB to 100 MB. Apple Silicon only. No sign-in, no telemetry.

Maintained by Sebastian Katz. Not affiliated with or endorsed by Warp Dev, Inc.

## What this is

Warp's terminal: blocks, block list, themes, Warpify shell integration, local
completions, workflows, keybindings, settings. Without the product around it.

## What this isn't

- **Not a supported build.** Ad-hoc signed, not notarized. macOS gates first launch.
- **Not upstream-compatible.** Whole subsystems are deleted. Rebasing gets harder
  over time. That's the trade.
- **Not a smaller Warp with the features intact.** Sign-in, the agent, Warp Drive,
  cloud sessions, settings sync, notifications, and the onboarding panel are gone,
  not hidden.
- **Not cross-platform.** macOS arm64 only. Linux and Windows paths are untouched
  but unexercised.
- **Not audited.** Telemetry and crash reporting are hard-off and verified in the
  logs, but this is one person's fork.

## Current state

| | Stock Warp | WarpNine |
|---|---|---|
| App bundle | 857 MB | **100 MB** |
| Architectures | x86_64 + arm64 | arm64 only |
| `__text` (code) | 126 MB | 54 MB |
| `__const` (data) | 167 MB | 42 MB |
| Unwinding tables | 25 MB | 0.3 MB |
| `__LINKEDIT` (symbols) | ~75 MB | 1.4 MB |
| Login wall | yes | no |
| Telemetry | yes | no |
| Dock tile plugin | 4.4 MB, universal | removed |

Launches to a shell. `--version` reports a real build stamp.

## Where the size was

The agent is not why Warp is big. Attributing symbol sizes in the stock binary to
their source modules puts the entire agent tree (`warp::ai` plus the `ai`, `rmcp`,
`candle`, `tantivy`, `computer_use`, `input_classifier`, `tokenizers` crates) at
roughly 22 MB of a 395 MB arm64 slice.

What actually cost the megabytes:

| Lever | Saved | Change |
|---|---|---|
| No x86_64 slice | 426 MB | one default flipped |
| `debug = 0` + `strip = "symbols"` | ~90 MB | upstream keeps line tables for Sentry |
| Dropped 34 tree-sitter grammars | 51 MB | `arborium` features |
| Excluded onboarding imagery | 47 MB | `rust-embed` inlined 57 MB of PNGs uncompressed |
| `opt-level = "s"` | 26 MB | release defaults to `3`, which optimizes for speed |
| `panic = "abort"` | 25 MB | removes `__eh_frame` / `__gcc_except_tab` |
| Dropped the ONNX classifier | 17.5 MB | `bert_tiny_v3.onnx`, embedded to route shell-vs-agent input |
| Dropped 663 PowerShell specs | 5 MB | dead weight on macOS |

Two of those are worth knowing about.

**41 MB of the binary was onboarding screenshots.** `crates/warp_assets` embeds
`app/assets/async` verbatim, no compression. The WASM and headless-CLI builds
already excluded that folder. Nobody had done it for the GUI build. The images are
reachable only from the login wall and three agent launch modals, all dead code
here.

**The grammars weren't for the terminal.** All 38 `arborium` languages served
Warp's code editor and the agent's codebase indexer. Terminal input highlighting
lives in `terminal/input/decorations.rs` and uses the completer's shell tokenizer.

### On Warp's feature flags

Warp's cargo features are runtime flags, not compile gates.
`app/src/features.rs` maps them onto `FeatureFlag::set_enabled()` booleans, and
`agent_mode` appears as a `#[cfg(feature = ...)]` gate exactly once in the tree.

Turning a feature off usually hides UI while still compiling it in. An earlier
version of this README concluded from that: "there is no configuration that makes
this smaller, only deletion does." That was wrong. The table above is the
refutation. Build-profile and asset-embedding changes account for most of the
reduction and delete no application code.

Deletion is still the only thing that removes the agent. It isn't what removes the
megabytes.

## What's removed

- **Sign-in and onboarding.** `root_view.rs` branched between a login wall, agent
  onboarding, and the terminal. It now always boots to the terminal. The startup
  Keychain read is gone too; it raised an OS prompt on every freshly signed build.
- **Telemetry and crash reporting.** Hard-off in `settings/privacy.rs`. Upstream's
  `should_disable_telemetry()` could be overridden by a force flag or the
  `AgentModeAnalytics` experiment. Both are ignored.
- **Agent UI:** settings pages, menu-bar entries, toasts, the tab-bar account menu,
  the agent notification inbox.
- **Warp Essentials** (`app/src/resource_center`, 3.4K LOC): onboarding tips,
  changelog feed, Docs/Slack/Feedback links, and every route into it.
- **The dock tile plugin.** It swapped the Dock icon among alternates, cost 4.4 MB,
  shipped universal regardless of target arch, and wrote a fresh
  `/tmp/warp_docktile_<timestamp>.log` on every init.
- **Referral and changelog menu items**, one of which rendered as
  `<NO DESCRIPTION>` because its label lookup no longer resolved.

## What's kept

Terminal and blocks, themes, Warpify, local completions (496 command specs),
workflows, keybindings, settings, the code editor (unhighlighted, see grammars
above), secret detection in terminal output.

## In progress

Deleting the agent code: `app/src/ai` (~220K LOC) plus the `ai`, `mcp`,
`computer_use`, `input_classifier` crates. Worth ~22 MB and a lot of mechanical
work. `terminal/` and `ai/blocklist/` import each other, so there's no small first
step. See [`EXCISION_MANIFEST.md`](EXCISION_MANIFEST.md).

`script/fork_separability` predicts those cascades. Distrust its name-frequency
signal: it over-reports on generic identifiers, and once predicted a 24-file
cascade for a type with zero external users.

## Building

Needs the pinned toolchain from `rust-toolchain.toml` (1.92.0). Homebrew's Rust
ignores the pin.

```bash
rustup toolchain install 1.92.0
cargo install cargo-bundle --git=https://github.com/burtonageo/cargo-bundle \
  --rev ae4c76e92c08774bf54ff077b1c52e3d1cd6c16d
cargo install --locked cargo-about@0.8.4

export PATH="$HOME/.cargo/bin:$PATH"
./script/macos/bundle --channel oss
```

Output: `target/aarch64-apple-darwin/release-lto/bundle/osx/WarpNine.app`

`cargo-about` matters more than it looks. It generates
`THIRD_PARTY_LICENSES.txt`, and that step runs before code signing. When it was
missing, the bundle script aborted there and never signed, leaving a bundle whose
executable carried the linker's ad-hoc signature while its resources were
unsealed. macOS treats that as corrupt rather than unsigned.
`prepare_bundled_resources` now warns and continues, and local builds get an
ad-hoc signature.

Verify:

```bash
codesign --verify --strict target/aarch64-apple-darwin/release-lto/bundle/osx/WarpNine.app
lipo -info target/aarch64-apple-darwin/release-lto/bundle/osx/WarpNine.app/Contents/MacOS/warp-nine
# Non-fat file: ... is architecture: arm64
```

Builds are slow. `lto = "fat"` with `codegen-units = 1` makes LLVM optimize the
whole program as one largely single-threaded unit. Tens of minutes is normal. Use
`lto = "thin"` in `[profile.release-lto]` for faster iteration.

`./script/bootstrap` works but installs tooling this fork doesn't need (Docker,
gcloud, PowerShell, Sentry CLI).

## Layout

```
origin    https://github.com/dxmachina/warp-nine.git
upstream  https://github.com/warpdotdev/warp.git
```

| Branch | Purpose |
|---|---|
| `main` | the fork |
| `master` | mirrors upstream, for rebasing |
| `local/deagent-terminal-ui` | agent excision WIP, **does not compile** |

```bash
git fetch upstream && git rebase upstream/master
```

## License

Unchanged from upstream: [AGPL v3](LICENSE-AGPL), except `warpui` and
`warpui_core`, which are [MIT](LICENSE-MIT). Upstream's copyright notices are
preserved in the bundle metadata and About page, with this fork credited
alongside.

`crates/warp_command_signatures` and `crates/warp_completion_metadata` are
vendored from
[warpdotdev/command-signatures](https://github.com/warpdotdev/command-signatures)
at `29cd61c3` with the PowerShell specs removed. `rust-embed`'s `folder` and
`exclude` attributes are compile-time literals, so a subtree can't be excluded
from outside the crate.

Upstream docs: [warpdotdev/warp](https://github.com/warpdotdev/warp).
