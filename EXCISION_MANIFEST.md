# Warp de-bloat / de-cloud excision manifest

Author: Sebastian Katz

Working notes for stripping login, agents, and cloud from the Warp OSS checkout
(`warp/`, upstream `warpdotdev/warp` @ `3fdb2dec6`), targeting an
Apple-Silicon-only local build.

## Correction (2026-07-30): the agent was not the size problem

This document was written assuming the agent and cloud code were why the binary
is large, and much of what follows is planning for that deletion. The assumption
was wrong, and the numbers below are the refutation. The plan is still valid as a
*de-agenting* plan. It is not the way to make the binary small.

Measured by attributing symbol sizes in the stock arm64 slice to source modules:

| Bucket | Size |
|---|---|
| `warp::ai` module | 11.8 MB |
| `rmcp`, `candle`, `ai`, `tantivy`, `input_classifier`, `computer_use`, `tokenizers` | 10.0 MB |
| **entire agent subsystem** | **~22 MB of a 395 MB slice** |

What actually accounted for the reduction, none of which deletes application
code:

| Lever | Saved |
|---|---|
| No x86_64 slice | 426 MB |
| `debug = 0` + `strip = "symbols"` | ~90 MB |
| Dropped 34 tree-sitter grammars (`arborium` features) | 51 MB |
| Excluded onboarding imagery from `rust-embed` | 47 MB |
| `opt-level = "s"` (release defaults to `3`) | 26 MB |
| `panic = "abort"` | 25 MB |
| Dropped `nld_classifier_v3` (`bert_tiny_v3.onnx`) | 17.5 MB |
| Dropped 663 PowerShell completion specs | 5 MB |

Result: 857 MB → 100 MB without finishing the agent excision.

Two specifics worth carrying forward:

- **41 MB of the binary was onboarding PNGs.** `crates/warp_assets` embeds
  `app/assets/async` verbatim with no compression. WASM and the headless CLI
  already excluded that folder; the GUI build never did.
- **The 38 tree-sitter grammars served the code editor and the agent's codebase
  indexer, not the terminal.** Terminal input highlighting is in
  `terminal/input/decorations.rs` and uses the completer's shell tokenizer.

Also note the earlier claim in this repo that "Warp's features are runtime flags,
so no configuration makes the binary smaller, only deletion does." The first half
is true. The conclusion is false: build-profile and asset-embedding changes did
most of the work.

## Correction (2026-07-30, second pass): `ai/blocklist` is agent code

An earlier note in this file held that `app/src/ai/blocklist` was the terminal's
block list, that it and `terminal/` imported each other, and that the block list
therefore had to be extracted before `app/src/ai` could be deleted. That was the
stated reason the excision had "no small first step." It is wrong.

- `terminal/` owns its block list outright: `terminal/model/block.rs`,
  `model/blocks.rs`, `model/blockgrid.rs`, `block_list_element.rs`,
  `block_list_viewport.rs`, `blockgrid_element.rs`, `blockgrid_renderer.rs`.
- `ai/blocklist` is the **agent conversation** list. Its `mod.rs` is 133 lines of
  nothing but `pub mod` / `pub use`, and every exported name is an agent type:
  `AIActionStatus`, `RunAgentsExecutor`, `BlocklistAIController`,
  `BlocklistAIPermissions`, `QueuedQueryModel`, `inherit_child_agent_settings`.
- `BlocklistAIHistoryModel`, despite being reached from `terminal/`, holds
  `HashMap<AIConversationId, AIConversation>` keyed by terminal surface. It is
  agent state hanging off a terminal surface, not terminal state.
- `blocklist/persistence.rs:587` carries an upstream `TODO(roland)` noting that
  `SerializedBlockListItem` now has a single `Command` variant because the
  serialized AI block is already gone.

So there is no extraction step. `app/src/ai` comes out wholesale.

### Sizing the repair, not the deletion

The 220K LOC figure measures what gets deleted, which is the easy part (one
`rm`). The work is the references left behind:

| | |
|---|---|
| `app/src/ai` | 219,959 LOC, 441 files (54,990 LOC in 123 `*_tests.rs`) |
| Agent crates | `ai` 30,744, `computer_use` 11,834, `mcp` 2,691, `input_classifier` 2,034 |
| **Reference lines outside `app/src/ai`** | **1,169 across 322 files** |

Concentration of those 1,169: `terminal/` 535, `workspace/` 121, `pane_group/`
105, `server/` 62, `settings_view/` 45, `search/` 23, `drive/` 15,
`code_review/` 14. Compare the 912-errors-across-268-files measurement below;
the two agree in magnitude.

### Warp Drive is not separable the way the agent is

Deleting `app/src/drive` looked attractive because Warp Drive needs the sign-in
this fork removed, so the feature is already dead. The code is not. `drive/`
hosts the object model *and* the editing UI for objects this fork keeps:

- `app/src/workflows` uses `drive::workflows::workflow_arg_selector`,
  `workflow_arg_type_helpers`, `arguments::ArgumentsState`,
  `enum_creation_dialog::WorkflowEnumData`, `items::WarpDriveWorkflow`.
- `app/src/env_vars` uses `drive::items::env_var_collection`, `drive::sharing`,
  `drive::export::ExportManager`.

Workflows and environment variables are kept features. So Drive needs the
carve-out that the agent turned out not to need: lift the workflow and env-var
item models and their argument UI into `workflows/` and `env_vars/` first, then
delete the cloud sync and sharing layers around them. Do it after the agent, as
its own pass, and do not bundle the two.

## Measuring progress during the excision (2026-07-30)

**The error count is meaningless while any parse-level error remains.** An
unclosed delimiter, a broken `macro_rules!` invocation, or an `E0583` file-not-
found halts rustc before name resolution, so cargo reports a handful of errors
while thousands go unseen. During this session the count read 2448, then 189,
then 1923, then 189, then 11, then 1923 again, entirely from blockers appearing
and clearing. None of those swings were work.

Always measure both numbers together:

```bash
cargo check --bin warp-nine --features release_bundle,extern_plist,gui \
  --message-format short 2>&1 | grep -E "error(\[|:)" > /tmp/e.txt
wc -l < /tmp/e.txt
grep -cE 'unclosed delimiter|mismatched closing|unexpected closing|no rules expected|end of macro|E0583' /tmp/e.txt
```

Only a total taken with a blocker count of zero means anything.

To find *every* broken file at once rather than one per compile, use rustfmt: it
parses each file independently instead of stopping at the first failure.

```bash
git diff --name-only main -- 'app/src/*.rs' | while read f; do
  [ -f "$f" ] && ! rustfmt --edition 2024 --emit stdout "$f" >/dev/null 2>&1 && echo "$f"
done
```

### What the automated passes could and could not do

Line-level tooling took this from ~4,000 errors to 1,171 with zero blockers.
Three transforms worked, in this order:

1. Delete functions that are agent surfaces. Require BOTH an agent-specific name
   and a live error in the body. Errors cluster hard: `handle_ai_history_model_event`
   was 533 lines, `handle_ai_block_event` 329. This removed 389 functions.
2. Delete struct fields and enum variants whose type is gone. 671 and 99.
3. Remove `use` statements that resolve to nothing.

Then they stopped helping, and kept going would have made things worse:

- Running (1)+(2)+(3) in a loop oscillated 1171 → 2235 → 1189 → 1680. Each pass
  broke files, a guard reverted them, and the reverted errors came back.
- Transform (3) alone took 1171 → **2338**. By this point the surviving `use`
  statements mix deleted names with valid ones, so removing whole statements
  creates more errors than it clears.

The remaining 1,171 need hand edits. There is no further mechanical lever.

### Two traps in the tooling itself

**Do not count `<` and `>` as delimiters** when finding the end of a
declaration. Every `->` in a signature decrements the depth, the scan overruns,
and it eats closing lines. This corrupted 15 files before it was caught.

**Do not identify a function by name plus indentation.** It is not unique. An
attempt to restore individual damaged functions from `main` spliced a 3-line
`new()` over a 143-line one in `left_panel.rs`.

When a file is damaged beyond a localised fix, restore the whole file from
`main` and let its agent references come back as ordinary name errors. That is
strictly better than leaving syntax damage, and it is what finally produced a
trustworthy measurement.

## On `script/fork_separability`

The tool's foreign-`impl` signal is sound and caught real traps. Its
name-frequency signal is not: it predicted a 24-file cascade for
`resource_center::ContentItem`, which had **zero** external users, because it was
matching a common word rather than a resolved symbol. Actual cascade when the
module was deleted: 40 errors, 27 of them missing imports in a file that had just
been extracted. Treat high counts on generic identifiers as unverified.

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
| `input_classifier` | 2,034 | ONNX models — see note below; not in `script/run` builds |
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

## What is actually in `__const` (148 MB)

Corrected after measuring, because the first guess was wrong.

**The three ONNX models are not in this build.** `crates/input_classifier`
embeds `bert_tiny_v{1,2,3}.onnx` (~51 MB) via `rust-embed`, but only behind its
`onnx` feature, which is reached via `nld_classifier_v*`. `script/run` builds
with `FEATURES="gui"` only — it is `script/macos/bundle` that appends
`gui,nld_classifier_v3,nld_heuristic_v2`. So a `./script/run --release` bundle
never contained them, and removing `input_classifier` will not reclaim 51 MB
from it. (A `script/bundle` artifact would.)

What is embedded in a `gui`-only build:

| Source | Size |
|---|---|
| `warp-command-signatures` (Fig completion specs, via `rust-embed` 6.8.1) | 31 MB |
| `languages/grammars` (via `rust-embed` 8.7.2) | 352 KB |

The balance is compiler-generated static data across 1.6M LOC of
generic-heavy UI code — it shrinks roughly in proportion to deleted code
rather than in one cut.

### Decision: keep the completion specs (2026-07-29)

The 488 Fig specs stay, all of them. They cost ~25-30 MB of uncompressed JSON
in `__const` (`rust-embed` is configured without a compression feature), which
is ~10% of the current binary and will be a much larger share of a smaller one.
Kept anyway: the completion dropdown is the main reason to run Warp over a
plain terminal, and this fork exists to remove the agent, not the terminal.

Do not re-open this without a reason. If it ever needs revisiting, the levers
are:

- `embed-signatures` is a feature, not a hard dep — upstream already builds
  without it for wasm (`app/Cargo.toml`, the `cfg(target_family = "wasm")`
  dependency block). Dropping it is a supported configuration.
- Trimming to a subset means vendoring `warp-command-signatures` into
  `crates/` (it is a git dependency) and pruning its `json/` folder. The size
  is a long tail — the largest single spec is `mongocli.json` at 1.2 MB, and
  488 files average ~60 KB — so a useful trim means picking tools, not
  deleting a few hogs.
- `classic_completions` and `force_classic_completions` are in the default
  feature set; `completions_v2` (which uses the local
  `crates/command-signatures-v2`) is not.

## The agent excision: measured plan

Cutting `mod ai;` from `lib.rs` produces **912 errors across 268 files**
(measured, not estimated). Concentrated in the integration hubs: `lib.rs` (48),
`terminal/view.rs` (45), `workspace/view.rs` (42), `terminal/input.rs` (38).

**Agent state is in the core data model, not just the UI.** Agent concepts reach
`persistence/sqlite.rs`, `server/graphql/schema/mod.rs`, `server/sync_queue.rs`,
`tab.rs`, `launch_configs/launch_config.rs`, `app_state.rs`, and
`cloud_object/model/persistence.rs` — session restore and tab configs serialize
agent conversations. So this is not a module extraction; part of it is schema
and serialization surgery.

### Delete `app/src/ai` submodules in this order

Cascade counts are consumers *outside the submodule itself*, split by whether
they live inside `app/src/ai`. Ascending cascade = correct deletion order.

| Module | LOC | sites | outside | inside `ai` |
|---|---|---|---|---|
| `ai/generate_block_title` | 13 | 2 | 2 | 0 |
| `ai/generate_code_review_content` | 25 | 2 | 2 | 0 |
| `ai/cloud_agent_config` | 58 | 1 | 1 | 0 |
| `ai/voice` | 160 | 2 | 2 | 0 |
| `ai/loading` | 45 | 3 | 2 | 1 |
| `ai/outline` | 498 | 9 | 7 | 2 |
| `ai/get_relevant_files` | 1,009 | 8 | 4 | 4 |
| `ai/conversation_navigation` | 349 | 9 | 5 | 4 |
| `ai/agent_events` | 1,846 | 7 | **0** | 7 |
| `ai/predict` | 1,707 | 16 | 13 | 3 |
| `ai/orchestration` | 3,003 | 8 | 3 | 5 |
| **`ai/agent_sdk`** | **38,674** | **16** | 12 | 4 |
| `ai/agent_management` | 7,258 | 14 | 12 | 2 |
| `ai/facts` | 2,094 | 26 | 22 | 4 |
| `ai/artifacts` | 1,073 | 33 | 8 | 25 |
| `ai/cloud_environments` | 426 | 44 | 22 | 22 |
| `ai/agent_conversations_model` | 739 | 56 | 40 | 16 |
| `ai/document` | 3,743 | 64 | 36 | 28 |
| `ai/skills` | 8,269 | 77 | 35 | 42 |
| `ai/mcp` | 8,016 | 83 | 46 | 37 |
| `ai/execution_profiles` | 8,439 | 83 | 50 | 33 |
| `ai/ambient_agents` | 3,243 | 193 | 95 | 98 |
| `ai/agent` | 24,616 | 680 | 238 | 442 |
| `ai/blocklist` | **121,932** | 436 | 381 | 55 |

**`ai/agent_sdk` is the standout: 38,674 LOC — 15% of the agent system — for
16 consumers.** Do it first. Its 12 external consumers are mostly pure-agent
files that go with it: `pane_group/pane/local_harness_launch.rs` (284),
`server/server_api/harness_support.rs` (498), `remote_server/handoff_snapshot.rs`
(67), `ai/bedrock_credentials.rs` (174), `terminal/view/docker_sandbox/mod.rs`
(338). The genuinely mixed consumers need only 1–4 edits each: `lib.rs` (a CLI
dispatch to `agent_sdk::run`), `workspace/view.rs` (claude/codex transcript
rehydration), `ai/blocklist/controller.rs` (`ClaudeHarness::wake_dormant_session`),
`ai/blocklist/action_model/recording_finalize.rs` (artifact upload), and
`ai/blocklist/handoff/snapshot.rs`.

Caution when grepping for those consumer files: several identifiers are
ambiguous. `docker_sandbox` matches both `terminal/view/docker_sandbox` and an
unrelated `terminal/local_tty/docker_sandbox`; `bedrock_credentials` matches
settings keys as well as the module; `handoff_snapshot` matches `server_api`
method names. Match on the module path, not the bare identifier.

### What `agent_sdk` taught us (and what stopped working after it)

`agent_sdk` came out cleanly because it was a genuine **leaf**: nothing inside
`app/src/ai` depended on it much (4 internal consumers, all reducible to
explicit failures). That property, not its size, is what made it removable.

Two follow-up attempts were reverted, and the reason generalises:

**`ai/orchestration` (3,003 LOC, 16 usages) — reverted.** Its consumers live
*inside* `ai/blocklist` (`action_model/execute/run_agents.rs`,
`handoff/pipeline.rs`, `inline_action/orchestration_controls.rs`,
`inline_action/run_agents_card_view.rs`). `app/src/ai` is a tightly-coupled
cluster centred on `blocklist`, so a low external cascade does not imply
separability. **Check internal consumers, not just external ones.** After
`agent_sdk`, the only remaining true leaves (zero internal consumers) are tiny:
`cloud_agent_config` (58), `generate_block_title` (13),
`generate_code_review_content` (25), `voice` (160).

**`terminal/view/use_agent_footer` (2,259 LOC) — reverted.** 43 errors across 6
files, because the module exports an `impl TerminalView` block whose methods
(`open_cli_agent_rich_input`, `maybe_show_use_agent_footer_in_blocklist`,
`has_active_cli_agent_input_session`, …) are called throughout
`terminal/view.rs`, `shared_session/shared_handlers.rs`, `pane_group/pane/mod.rs`
and `local_tty/terminal_view_adaptor.rs`. It also contains
`warpify_footer.rs` — **Warpify is shell setup, not an agent feature** — and a
generic `is_running_warp_tui` helper.

### The trap to watch for

Warp's terminal-view modules bundle agent and non-agent UI in the same module.
This has now bitten twice, and the compiler caught it both times:

| Module | Agent part | Must survive |
|---|---|---|
| `terminal/view/docker_sandbox` | `initialize_docker_sandbox_environment` (agent env driver) | `create_and_push_docker_sandbox` — user-facing sandbox pane behind `FeatureFlag::LocalDockerSandbox` |
| `terminal/view/use_agent_footer` | `UseAgentToolbar` | `warpify_footer.rs`, `is_running_warp_tui` |

So the remaining work is **per-function surgery inside shared modules**, not
module deletion. That is a slower mode than the `agent_sdk` slice — budget
accordingly, and expect the module's LOC count to overstate what can actually
be removed.

Recommended next direction: rather than deleting further `app/src/ai`
submodules from the inside, remove `ai::`'s **external** consumers first (the
386 files / 1,255 imports outside `app/src/ai`) so the cluster can eventually
come out as a unit. Start with modules that are wholly agent with no shared
helpers — verify by reading the module's exports before deleting, not by
grepping its name.

### The phases are not independent (measured)

An attempt to cut the terminal↔AI-block seam on its own established the real
shape of the remaining work. Numbers below are measured, not estimated.

Removing the agent variants of `RichContentMetadata` / `RichContentType` alone:
**117 errors**. Then deleting the 13 wholly-agent functions in
`terminal/view.rs` (`remove_ai_blocks_for_exchanges`,
`handle_ai_history_model_event`, `toggle_usage_footer`,
`cleanup_and_remove_conversation_for_ai_block`, `drop_hidden_passive_ai_blocks`,
`active_ai_block`, `last_ai_block`, `rewind_ai_conversation`,
`is_any_ai_block_focused`, `ai_block_metadata_for_current_thread`,
`handle_ai_controller_event`, `handle_cli_subagent_controller_event`,
`handle_usage_footer_toggled`) took it **up to 132**, because their callers
broke.

Only 3 of those 166 error sites were inside `app/src/ai`. The work is in
`terminal/`, so deleting `app/src/ai` first does not help.

**It cannot be staged smaller.** `terminal/view/agent_view.rs`,
`terminal/view/pending_user_query.rs`, `terminal/view/ambient_agent/` and
`terminal/view/load_ai_conversation.rs` all *construct* the removed variants, so
they must come out in the same change. The minimum atomic unit is roughly
15–20K LOC:

- `terminal/view/rich_content.rs` — agent variants, `AIBlockMetadata`,
  `AgentViewEntryMetadata`, `RichContent::agent_view_conversation_id`
- `terminal/model/rich_content.rs` — agent `RichContentType` variants
- `terminal/view.rs` — 13 functions plus ~20 mixed call sites across
  `context_menu_action`, `new`, `dismiss_tooltips`,
  `context_color_for_rich_content`, `rerender_rich_content_blocks`,
  `clear_selected_text_except`, `handle_session_bootstrapped`
- `terminal/view/{agent_view, pending_user_query, load_ai_conversation}.rs`
- `terminal/view/ambient_agent/` (10,134 LOC, 96 externally-referenced names)
- `terminal/view/context_menu.rs`, `terminal/block_list_element.rs`,
  `terminal/model/blocks.rs`

Must survive: `RichContentMetadata::{InitStep, InitEnvironment,
EnvVarCollectionBlock, SshRemoteServerChoiceBlock, SshRemoteServerFailedBanner,
SshTmuxDeprecationBanner, WarpifySuccessBlock, TelemetryBanner,
TerminalViewZeroState, PluginInstructionsBlock}` and
`RichContentType::{WarpifySuccessBlock, TerminalViewZeroState,
PluginInstructionsBlock}`.

Practical consequence: do this on a scratch branch, not on
`local/slim-arm64-no-cloud`. The tree does not compile part-way through, so the
usual "commit at every green checkpoint" discipline does not apply within the
change — there is no green checkpoint until it is finished.

### Leave for last

`ai/blocklist` (121,932 LOC, 381 external sites) and `ai/agent` (24,616 LOC,
238 external sites) are the load-bearing ones. `RichContentType` in
`terminal/model/rich_content.rs` is the seam where agent blocks enter the
terminal's block list — its `AIBlock`, `EnterAgentView`, `InlineAgentViewHeader`,
and `AgentViewZeroState` variants are the agent side;
`WarpifySuccessBlock`, `TerminalViewZeroState`, and `PluginInstructionsBlock`
are not and must survive.

### Pure-agent modules outside `app/src/ai` (~40K LOC)

`terminal/view/ambient_agent` (10,142), `search/ai_context_menu` (7,527),
`terminal/cli_agent_sessions` (4,568), `ai_assistant` (3,637),
`integration_testing/agent_mode` (2,461), `terminal/view/use_agent_footer`
(2,259), `workspace/view/conversation_list` (2,198), `pane_group/child_agent`
(1,084), `terminal/view/load_ai_conversation.rs` (1,176).

Note `search/ai_context_menu` is the "@" menu and is woven into
`terminal/input.rs`'s editor — more entangled than its file count suggests.

## Excision order (dependents before dependencies)

1. Leaf UI: settings pages, menus, command palette, onboarding slides.
   - [x] `referrals_page` — 1,357 lines across 10 files. Note the shape: the
         page file was 1,125 of those; the rest was the enum variants, the
         `WorkspaceAction` and its six dispatch sites, two keybindings, an
         overflow-menu item, a resource-center button, and a settings widget.
         Budget similarly for every other page.
   - [ ] `warp_drive_page` — **not** a settings-page-sized job. Warp Drive is a
         left-panel subsystem touching `palette.rs`, `app_state.rs`,
         `root_view.rs`, `util/bindings.rs`, `auth/login_slide.rs`,
         `resource_center/mod.rs`, and `app/src/drive` (23K LOC). Do it in the
         cloud pass, not here.
   - [x] `ai_page` — 14,005 lines across 31 files. Deleting it orphaned five
         modals that only it constructed (`custom_inference_modal`,
         `execution_profile_view`, `custom_router_view`,
         `set_default_model_modal`,
         `remove_custom_endpoint_confirmation_dialog`), which went too.
   - [ ] Remaining pages, with measured external cascade (sites / files):

     | Page | LOC | Cascade |
     |---|---|---|
     | `teams_page` | 4,477 | 16 / 6 |
     | `billing_and_usage_page` + `_v2` | 5,857 | 39 / 10 |
     | `code_page` | 3,012 | 24 / 3 |
     | `environments_page` | 2,095 | 44 / 8 |
     | `mcp_servers_page` | 589 | 23 / 7 |
     | `warp_drive_page` | 282 | 15 / 2 |

     Measure the cascade before picking the next one — `ai_page` was the
     largest file but among the cheapest to remove, and `environments_page` is
     the opposite (16 of its 44 sites are in
     `pane_group/pane/environment_management_pane.rs`).
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
