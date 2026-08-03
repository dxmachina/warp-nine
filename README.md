# WarpNine

A personal fork of [Warp](https://github.com/warpdotdev/warp) with the account
system, coding agent, and cloud backend removed.

857 MB to 78 MB. Separate arm64 and Intel builds. No sign-in, no telemetry.

Maintained by Sebastian Katz. Not affiliated with or endorsed by Warp Dev, Inc.

## Download

| | For | Size | Minimum macOS |
|---|---|---|---|
| **[WarpNine-arm64.dmg](https://github.com/dxmachina/warp-nine/releases/latest/download/WarpNine-arm64.dmg)** | Apple Silicon (M1 and later) | 31 MB | 11.0 Big Sur |
| **[WarpNine-x86_64.dmg](https://github.com/dxmachina/warp-nine/releases/latest/download/WarpNine-x86_64.dmg)** | Intel | 34 MB | 10.14 Mojave |

Which one: Apple menu → About This Mac. "Apple M1" or later is arm64.

Open the image, drag WarpNine to Applications, launch it. Signed with a Developer
ID certificate and notarized by Apple, so it opens on a double click — no
Gatekeeper prompt, no right-click-Open dance.

To check what you got first:

```bash
shasum -a 256 ~/Downloads/WarpNine-arm64.dmg
spctl -a -vvv -t exec /Volumes/WarpNine/WarpNine.app
# source=Notarized Developer ID
```

Those links always resolve to the newest build. Per-build checksums and notes are
on the [release page](https://github.com/dxmachina/warp-nine/releases/latest).

## What this is

Warp's terminal: blocks, block list, themes, Warpify shell integration, local
completions, workflows, keybindings, settings. Without the product around it.

## What this isn't

- **Not a supported build.** Signed so it opens cleanly, not because anyone stands
  behind it.
- **Not upstream-compatible.** Whole subsystems are deleted. Rebasing gets harder
  over time. That's the trade.
- **Not a smaller Warp with the features intact.** Sign-in, the agent, Warp Drive,
  shared sessions, settings sync, notifications, and the onboarding panel are gone,
  not hidden.
- **Not cross-platform.** macOS only. Linux and Windows paths are untouched but
  unexercised. Both macOS architectures build, but only arm64 is run day to day;
  the Intel build is verified as far as a signed, notarized launch under Rosetta.
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

Measured at `4e8698693`; the breakdown rows below were measured at `3cb1ec862`.
Everything excised between them is worth ~2 MB, because deleting call-path code
removes lines, not bytes. The binary is dominated by dependencies that stay.

## Where the size was

The agent is not why Warp is big. Attributing symbol sizes in the stock binary to
their source modules puts the entire agent tree (`warp::ai` plus the `ai`, `rmcp`,
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
version of this README concluded "only deletion makes this smaller." The table
above refutes it: build-profile and asset-embedding changes account for most of
the reduction and delete no application code.

Deletion is still the only thing that removes the agent. It isn't what removes the
megabytes.

## What's removed

- **Sign-in and onboarding.** `root_view.rs` branched between a login wall, agent
  onboarding, and the terminal. It now always boots to the terminal. The startup
  Keychain read is gone too; it raised an OS prompt on every freshly signed build.
- **The onboarding crate** (~16.9K LOC): the guided tutorial, its callout view and
  keybinding builder, the HOA onboarding flow, and the Oz and OpenWarp launch
  modals. Only the *branch* had gone previously; the crate stayed linked, and the
  `agent_onboarding` and `account_first_onboarding` flags were still in the default
  feature set. Three separate things made it unreachable anyway: `AuthOnboardingState`
  has had one variant since the login wall went, every tutorial entry point sits
  behind `is_anonymous_or_logged_out()`, and `pending_onboarding_intention` had a
  single assignment guarded by it already being `Some`, so it could only ever be
  `None`. The Oz modal was the last live constructor of `OnboardingTutorial`, and
  took the generic `LaunchModal<S>` with it: `OzLaunchSlide` was its only `Slide`.
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
- **The agent** (~220K LOC): `app/src/ai` and the `ai`, `mcp`, `computer_use` and
  `input_classifier` crates.
- **The cloud object write path** (~20.6K LOC): the sync queue, the live GraphQL
  mutations, and the thirteen `UpdateManager` methods behind them. Five affordances
  were already dead before this: Trash, Untrash, permanent delete, the notebook
  edit baton, and drive sharing all sat behind a `let Some(server_id) = ... else
  { return; }` guard that a locally created object can never satisfy. The button
  stayed enabled and did nothing.
- **Shared sessions** (~19.1K LOC): the real-time collaborative terminal. A
  WebSocket to `wss://sessions.app.warp.dev` streamed scrollback, PTY reads,
  selections and presence to viewers who joined by link, with Reader / Executor /
  Full roles and a CRDT-shared input line. Sharing had already been unreachable
  since sign-in was closed off; joining someone else's session had not. Gone with
  it: the `session-sharing-protocol` dependency, five feature flags, and the
  `warp://shared_session/{id}` URI host.

## What's kept

Terminal and blocks, themes, Warpify, local completions (496 command specs),
workflows, keybindings, settings, the code editor (unhighlighted, see grammars
above), secret detection in terminal output.

## What's left

`remote_server` (~20.6K LOC across `app/src/remote_server`, `crates/remote_server`,
the SSH choice/failed-banner views, the PTY controller and the command executor) is
the last large subsystem that talks to Warp's infrastructure. It installs and drives
a Warp-built helper binary on an SSH host so blocks and Warpify work remotely.
Removing it degrades SSH to the wrapper-only warpification path, which stays.

A removal is part-done and parked as a patch, not committed: the deletions and the
`warp_files` half are complete (`FileBackend::Remote` is gone, and
`register_remote_file`, its only constructor, had no callers), but 74
unresolved-module errors across 24 files remain. All are `E0432`/`E0433`, so
mechanical in kind — but each site needs a decision about the code that used the
import, not just the import.

**Notebooks are not separable.** `app/src/notebooks/editor` is 14,340 of the
subsystem's 21,638 lines and is not notebook-specific: it is the app's rich-text
editor, imported by `code_review/comment_list_view.rs`, `code_review/comment_rendering.rs`,
`code/editor/comment_editor.rs`, `code/editor/{view,model}.rs` and `lib.rs`. It also
needs `notebooks::{file, link, styles, telemetry}` and `search::notebook_embedding`.
Removing notebooks whole would take the in-app git diff review with it. Either the
product surface goes and the editor is re-homed under a truthful name, or code review
goes too. That is a product decision.

`warp_server_client` (3,022 lines) and `warp_server_auth` (1,391) stay. They were
originally named as part of the cloud-object excision, but they are not gated on
it: they back `remote_server`, API key management, the multi-agent client, and
crash reporting.

See [`EXCISION_MANIFEST.md`](EXCISION_MANIFEST.md) for what each pass removed and
what it broke on the way.

`script/fork_separability` predicts removal cascades. Distrust its name-frequency
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

For a signed, notarized release, use `script/release` instead. It checks both
credentials before the build rather than after, then mounts the finished image,
copies the app out, applies the quarantine attribute a browser would set, and
confirms the extracted copy still validates.

```bash
./script/release                 # arm64
./script/release --arch x86_64   # Intel, cross-built; needs Rosetta for the smoke test
./script/release --arch both     # both, each fully verified before the next starts
./script/release --signed-only   # skip notarization
```

Disk images are named for their architecture and land in
`target/release-lto/bundle/osx`:

```
WarpNine-arm64.dmg      31 MB
WarpNine-x86_64.dmg     34 MB
```

That directory is not per-target, so the loose `.app` copy in it belongs to
whichever architecture built last. The one each image was built from stays under
`target/<rust-target>/release-lto/bundle/osx/`.

**Minimum macOS: 11.0 on arm64, 10.14 on Intel.** `.cargo/config.toml` sets
`MACOSX_DEPLOYMENT_TARGET = "10.14"`, which binds on Intel and is also what
`crates/warpui/build.rs` pins the Metal AIR bytecode to. On arm64 it cannot bind:
Rust's floor for Apple Silicon is 11.0 because Big Sur is where Apple Silicon
started. Nothing sets `LSMinimumSystemVersion`, so the Mach-O load command is the
only gate, and neither floor has been tested below the machine that builds them.
One feature sits higher and says so: the login item uses `SMAppService`, macOS 13+.

`cargo-about` matters more than it looks. It generates
`THIRD_PARTY_LICENSES.txt`, a step that runs *before* code signing. When it was
missing the bundle script aborted there and never signed, leaving an executable
with the linker's ad-hoc signature and unsealed resources — which macOS treats as
corrupt rather than unsigned. `prepare_bundled_resources` now warns and continues,
and local builds get an ad-hoc signature.

Verify:

```bash
codesign --verify --strict target/aarch64-apple-darwin/release-lto/bundle/osx/WarpNine.app
lipo -info target/aarch64-apple-darwin/release-lto/bundle/osx/WarpNine.app/Contents/MacOS/warp-nine
# Non-fat file: ... is architecture: arm64
```

Builds are slow. `lto = "fat"` with `codegen-units = 1` makes LLVM optimize the
whole program as one largely single-threaded unit. Tens of minutes is normal. Use
`lto = "thin"` in `[profile.release-lto]` for faster iteration.

`target/` gets large, for two reasons. Upstream generates the settings schema
without `--target`, so that helper binary lands in a separate artifact tree and
drags a second full copy of every dependency with it; the bundle script now passes
the triple through. And cargo never garbage-collects superseded artifacts, so each
rebuild of the `warp` lib crate leaves a ~600 MB `.rlib` behind. Delete all but the
newest when it gets out of hand, or use `cargo-sweep`.

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
