# WarpNine

A personal fork of [Warp](https://github.com/warpdotdev/warp) with the account
system, coding agent, and cloud backend removed. The app bundle drops from 857 MB to
78 MB. There is no sign-in and no telemetry.

Not affiliated with or endorsed by Warp Dev, Inc.

## Download

| | For | Size | Minimum OS |
|---|---|---|---|
| **[WarpNine-arm64.dmg](https://github.com/dxmachina/warp-nine/releases/latest/download/WarpNine-arm64.dmg)** | Apple Silicon | 31 MB | macOS 11.0 |
| **[WarpNine-x86_64.dmg](https://github.com/dxmachina/warp-nine/releases/latest/download/WarpNine-x86_64.dmg)** | Intel | 34 MB | macOS 10.14 |
| **[WarpNineSetup.exe](https://github.com/dxmachina/warp-nine/releases/download/v9.2026.08.18.11.41.f9b203958/WarpNineSetup.exe)** | Windows x64 | 35 MB | Windows 10 |

Open the image and drag WarpNine to Applications. The build is Developer ID signed
and notarized, so it opens without a Gatekeeper prompt. To verify a download, run
`spctl -a -vvv -t exec /Volumes/WarpNine/WarpNine.app` and check for
`source=Notarized Developer ID`. Per-build checksums are on the
[release page](https://github.com/dxmachina/warp-nine/releases/latest).

The Windows installer is not code-signed, so SmartScreen warns on first run
(More info, then Run anyway); its SHA-256 is on its release page. It installs
per-user by default (a dialog offers all-users), has only been exercised on one
Windows 11 machine, and is published as a pre-release pinned to its tag so the
macOS `latest` links above keep working.

## What it is

Warp's terminal: blocks and the block list, themes, Warpify shell integration, local
completions (496 command specs), workflows, keybindings, settings, the code editor
without syntax highlighting, and secret detection in terminal output.

Sign-in, the agent, Warp Drive, shared sessions, settings sync, notifications, and
onboarding are deleted rather than disabled.

## Limitations

- Nobody supports this build. It is signed so that it opens cleanly, not because
  anyone stands behind it.
- Whole subsystems are deleted, so rebasing on upstream gets harder over time.
- macOS is the primary platform. Both architectures build, but only arm64 is used
  daily. The Intel build has been verified as far as a notarized launch under
  Rosetta and has never run on Intel hardware. Windows x64 builds and runs
  (terminal sessions, blocks, session restore, and the Inno Setup installer all
  verified on one machine) but is unsigned and much less exercised. Linux code is
  present but unexercised.
- Telemetry and crash reporting are compiled out and verified in the logs, but this
  is one person's fork and has not been audited.

## Size

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

Measured at `4e8698693`. The lever table below was measured at `3cb1ec862`;
excisions between the two commits account for about 2 MB, because deleting
call-path code removes lines rather than bytes. Dependencies dominate the binary.

The agent is not the main cost. Attributing symbol sizes in the stock binary to
their source modules puts the whole agent tree (`warp::ai` and the `ai`, `rmcp`,
`candle`, `tantivy`, `computer_use`, `input_classifier`, `tokenizers` crates) at
about 22 MB of a 395 MB arm64 slice. What actually cost the megabytes:

| Lever | Saved | Change |
|---|---|---|
| No fat binary | 426 MB | one default flipped; each image carries one slice |
| `debug = 0` + `strip = "symbols"` | ~90 MB | upstream keeps line tables for Sentry |
| Dropped 34 tree-sitter grammars | 51 MB | `arborium` features |
| Excluded onboarding imagery | 47 MB | `rust-embed` inlined 57 MB of PNGs uncompressed |
| `opt-level = "s"` | 26 MB | release defaults to `3`, which optimizes for speed |
| `panic = "abort"` | 25 MB | removes `__eh_frame` / `__gcc_except_tab` |
| Dropped the ONNX classifier | 17.5 MB | `bert_tiny_v3.onnx`, embedded to route shell-vs-agent input |
| Dropped 663 PowerShell specs | 5 MB | unreachable on macOS |

The onboarding imagery came from `crates/warp_assets`, which embeds
`app/assets/async` verbatim and uncompressed; the WASM and headless-CLI builds
already excluded it and the GUI build did not. The tree-sitter grammars served the
code editor and the agent's codebase indexer rather than the terminal, whose input
highlighting uses the completer's shell tokenizer.

Configuration alone would not have done this. Warp's cargo features are runtime
flags rather than compile gates: `app/src/features.rs` maps them onto
`FeatureFlag::set_enabled()` booleans, and `agent_mode` is a `#[cfg(feature = ...)]`
gate once in the tree. Turning a feature off usually hides UI while still compiling
it in. Most of the reduction above is build-profile and asset changes, which delete
no application code.

## What was removed

| | LOC | |
|---|---|---|
| The agent | ~220K | `app/src/ai` and the `ai`, `mcp`, `computer_use`, `input_classifier` crates, plus its settings pages, menu entries, toasts, account menu, and notification inbox |
| The cloud object write path | ~20.6K | the sync queue, live GraphQL mutations, thirteen `UpdateManager` methods |
| Shared sessions | ~19.1K | the collaborative terminal: `wss://sessions.app.warp.dev`, Reader / Executor / Full roles, CRDT-shared input, the `warp://shared_session/{id}` URI host |
| The onboarding crate | ~16.9K | guided tutorial, callout view, keybinding builder, HOA flow, Oz and OpenWarp launch modals |
| Warp Essentials | 3.4K | `app/src/resource_center`: tips, changelog feed, Docs/Slack/Feedback links |
| Sign-in | | the login wall, and the startup Keychain read that raised an OS prompt on every freshly signed build |
| Telemetry and crash reporting | | hard-off in `settings/privacy.rs`; upstream's force flag and `AgentModeAnalytics` override are ignored |
| The dock tile plugin | | 4.4 MB, shipped universal regardless of target arch, wrote `/tmp/warp_docktile_<timestamp>.log` on every init |
| Referral and changelog menu items | | one rendered as `<NO DESCRIPTION>` because its label lookup no longer resolved |

Several were already unreachable. Trash, Untrash, permanent delete, the notebook
edit baton, and drive sharing all sat behind a
`let Some(server_id) = ... else { return; }` that a locally created object cannot
satisfy, so those buttons were enabled and did nothing. Onboarding was unreachable
by three separate mechanisms while still being a default feature.

## What remains

`remote_server` (~20.6K LOC) is the last large subsystem that contacts Warp's
infrastructure. It installs and runs a Warp-built helper on an SSH host so that
blocks and Warpify work remotely; removing it would reduce SSH to the wrapper-only
warpification path, which stays. A partial removal exists as an uncommitted patch,
with 74 unresolved-module errors across 24 files left. All are `E0432` or `E0433`,
so they are mechanical, but each site needs a decision about the code that used the
import.

Notebooks cannot be separated. `app/src/notebooks/editor` is 14,340 of the
subsystem's 21,638 lines and is the application's rich-text editor, imported across
`code_review/` and `code/editor/`, so removing notebooks would take the in-app git
diff review with it.

`warp_server_client` (3,022 lines) and `warp_server_auth` (1,391) stay. They were
named as part of the cloud-object excision but are not gated on it: they back
`remote_server`, API key management, and crash reporting.

[`EXCISION_MANIFEST.md`](EXCISION_MANIFEST.md) records what each pass removed and
what it broke. `script/fork_separability` predicts removal cascades, but its
name-frequency signal once predicted a 24-file cascade for a type with no external
users.

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
# -> target/aarch64-apple-darwin/release-lto/bundle/osx/WarpNine.app
```

`script/release` produces a signed and notarized build instead. It checks both
credentials before the build, then mounts the finished image, copies the app out,
applies the quarantine attribute a browser would set, and confirms that copy still
validates.

```bash
./script/release                 # arm64
./script/release --arch x86_64   # Intel, cross-built; smoke test needs Rosetta
./script/release --arch both     # both, each fully verified before the next starts
./script/release --signed-only   # skip notarization
```

Images land in `target/release-lto/bundle/osx` named for their architecture. That
directory is not per-target, so the loose `.app` beside them is whichever built
last. The app each image was built from stays under
`target/<rust-target>/release-lto/bundle/osx/`.

To check a local build:

```bash
BUNDLE=target/aarch64-apple-darwin/release-lto/bundle/osx/WarpNine.app
codesign --verify --strict "$BUNDLE"
lipo -archs "$BUNDLE/Contents/MacOS/warp-nine"   # arm64
```

**Minimum macOS: 11.0 arm64, 10.14 Intel.** `.cargo/config.toml` sets
`MACOSX_DEPLOYMENT_TARGET = "10.14"`, which `crates/warpui/build.rs` also pins the
Metal AIR bytecode to. It cannot apply on arm64, where Rust's floor is 11.0 because
Big Sur is the first release supporting Apple Silicon. Nothing sets
`LSMinimumSystemVersion`, so the Mach-O load command is the only gate, and neither
floor has been tested below the machine that builds them.

Three things that have cost time:

- `cargo-about` is required. It generates `THIRD_PARTY_LICENSES.txt` in a step that
  runs before signing, and when it was missing the script aborted there and never
  signed, leaving the linker's ad-hoc signature over unsealed resources, which macOS
  reads as corrupt rather than unsigned. `prepare_bundled_resources` now warns and
  continues.
- Builds are slow. `lto = "fat"` with `codegen-units = 1` optimizes the whole program
  as one largely single-threaded unit, so tens of minutes is normal. Use
  `lto = "thin"` in `[profile.release-lto]` to iterate.
- `target/` grows without bound, because cargo never collects superseded artifacts
  and every rebuild of the `warp` lib crate leaves a ~600 MB `.rlib`. Use
  `cargo-sweep`.

`./script/bootstrap` works but installs tooling this fork does not need (Docker,
gcloud, PowerShell, Sentry CLI).

### Windows

Needs the same pinned toolchain, plus MSVC (Visual Studio Build Tools or any
edition with the C++ workload), LLVM (for `libclang`, used by bindgen), protoc,
CMake, and Inno Setup for the installer. NASM is not needed if
`AWS_LC_SYS_PREBUILT_NASM=1` is set. With those on `PATH` (and `LIBCLANG_PATH`
pointing at LLVM's `bin`):

```powershell
.\script\windows\bundle.ps1 -CHANNEL oss
# -> script\windows\Output\WarpNineSetup.exe
```

The binary lands in `target\x86_64-pc-windows-msvc\rlto\warp-nine.exe`. To run it
from there rather than installing, copy `conpty.dll`, `dxcompiler.dll`,
`dxil.dll`, and `x64\OpenConsole.exe` in from `app\assets\windows\x64`; the
build script puts them in `target\rlto` instead because it does not account for
`--target` (noted in the manifest). The installer stages them from the assets
directory and is unaffected. The 663 PowerShell cmdlet completion specs removed
from the macOS build are compiled in on Windows only.

`.\script\windows\bootstrap.ps1` installs the dependency list above via winget
but also wants gcloud; this fork does not need it.

## Layout

`main` is the fork. `master` mirrors upstream for rebasing.

```bash
git remote add upstream https://github.com/warpdotdev/warp.git
git fetch upstream && git rebase upstream/master
```

## License

Unchanged from upstream: [AGPL v3](LICENSE-AGPL), except `warpui` and `warpui_core`,
which are [MIT](LICENSE-MIT). Both texts ship inside the app bundle at
`Contents/Resources`. Upstream's copyright notices are preserved in the bundle
metadata and the About page, with this fork credited alongside.

`crates/warp_command_signatures` and `crates/warp_completion_metadata` are vendored
from [warpdotdev/command-signatures](https://github.com/warpdotdev/command-signatures)
at `29cd61c3` with the PowerShell specs removed. `rust-embed`'s `folder` and
`exclude` attributes are compile-time literals, so a subtree cannot be excluded from
outside the crate.
