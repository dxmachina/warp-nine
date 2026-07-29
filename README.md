# warp-nine

A personal fork of [Warp](https://github.com/warpdotdev/warp) that strips out the
account system, the built-in coding agent, and the cloud backend — leaving the
terminal.

Maintained by Sebastian Katz. Not affiliated with or endorsed by Warp Dev, Inc.

## Why

Warp is a genuinely good terminal wrapped in things I don't want:

1. **Forced sign-in.** The stock client gates startup behind a Warp account.
2. **The agent.** An entire coding-agent product compiled into the terminal.
3. **Size.** The shipped `Warp.app` is **857 MB**. For a terminal.

That last number turns out to be mostly self-inflicted. Measured against the
stock `/Applications/Warp.app`:

| Component | Size |
|---|---|
| `Contents/MacOS/stable` | 840 MB |
| — x86_64 slice | 426 MB |
| — arm64 slice | 414 MB |
| Frameworks (Sentry) | 32 MB |
| Helpers (pprof) | 12 MB |
| Resources | 6.5 MB |
| PlugIns (DockTile, fat) | 4.4 MB |

Half the binary is an Intel slice that does nothing on Apple Silicon, and
another 90 MB is a symbol table kept for crash-report symbolication.

## Current state

**857 MB → 281 MB**, verified running.

| | Stock | This fork |
|---|---|---|
| App bundle | 857 MB | **281 MB** |
| Main binary | 840 MB (fat) | 292 MB (arm64) |
| `__LINKEDIT` (symbols) | 90 MB | 3.5 MB |
| Architectures | x86_64 + arm64 | arm64 only |
| Login wall | yes | no |
| Telemetry on startup | yes | no |

The remaining 292 MB is `__text` 111 MB + `__const` 148 MB — that's the agent
and cloud code, and it only shrinks by deleting it. That work is in progress;
see [`EXCISION_MANIFEST.md`](EXCISION_MANIFEST.md).

### Done

- **Apple-Silicon-only builds.** `script/macos/bundle` defaults to arm64;
  `--universal` opts back in.
- **Symbols stripped.** `[profile.release]` uses `debug = 0` + `strip = "symbols"`
  instead of upstream's `debug = 1`.
- **No login, no onboarding.** The startup gate in `root_view.rs` branched
  between a login wall, the agent onboarding flow, and the terminal. It now
  always boots to the terminal.
- **No telemetry.** Hard-off in `settings/privacy.rs`. Upstream's
  `should_disable_telemetry()` could be overridden by a force flag or the
  `AgentModeAnalytics` experiment; both are ignored here.
- **Agent UI removed** from the settings sidebar and the menu bar.

### In progress

Deleting the code itself: `app/src/ai` (261K LOC), `app/src/auth`,
`app/src/drive`, and the `ai` / `warp_server_auth` / `warp_server_client` /
`firebase` / `cloud_object_*` / `input_classifier` crates. The last of those
embeds three ONNX models (~51 MB) via `rust-embed`.

## A note on Warp's feature flags

Worth recording, because it's counterintuitive and it determines what is
actually possible here: **Warp's cargo features are runtime flags, not compile
gates.** `app/src/features.rs` maps them onto `FeatureFlag::set_enabled()`
booleans, and `agent_mode` appears as a `#[cfg(feature = ...)]` gate exactly
once in the whole tree.

So turning features off hides UI but compiles every line of it into the binary.
There is no configuration that makes this smaller. Only deletion does.

## Building

Requires the pinned toolchain from `rust-toolchain.toml` (1.92.0) — Homebrew's
Rust will not do, since it ignores the pin.

```bash
rustup toolchain install 1.92.0
cargo install cargo-bundle --git=https://github.com/burtonageo/cargo-bundle \
  --rev ae4c76e92c08774bf54ff077b1c52e3d1cd6c16d

export PATH="$HOME/.cargo/bin:$PATH"
./script/run --release --dont-open
```

Output: `target/release/bundle/osx/WarpOss.app`.

Verify it is single-architecture:

```bash
lipo -info target/release/bundle/osx/WarpOss.app/Contents/MacOS/warp-oss
# Non-fat file: ... is architecture: arm64
```

`./script/bootstrap` also works but installs a large amount of tooling
(Docker, gcloud, PowerShell, Sentry CLI) that this fork does not need.

macOS only. The Linux and Windows build paths are untouched but unexercised.

## Tracking upstream

```
origin    https://github.com/dxmachina/warp-nine.git
upstream  https://github.com/warpdotdev/warp.git
```

`master` mirrors upstream; the fork lives on `local/slim-arm64-no-cloud`.

```bash
git fetch upstream && git rebase upstream/master
```

Expect this to get harder as deletion proceeds — that is the trade being made
deliberately, in exchange for a binary that doesn't contain a coding agent.

## License

Unchanged from upstream: [AGPL v3](LICENSE-AGPL), except the `warpui` and
`warpui_core` crates, which are [MIT](LICENSE-MIT).

For upstream's own documentation, see
[warpdotdev/warp](https://github.com/warpdotdev/warp).
