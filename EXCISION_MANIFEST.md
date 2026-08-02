# Warp de-bloat / de-cloud excision manifest

Author: Sebastian Katz

Working notes for stripping login, agents, and cloud from the Warp OSS checkout
(`warp/`, upstream `warpdotdev/warp` @ `3fdb2dec6`), targeting an
Apple-Silicon-only local build.

## Login is gone (2026-07-31)

`app/src/auth` is down to a single 25-line `mod.rs` that re-exports account
*state*. 62 files, 7,578 deletions. What went:

| file | lines |
|---|---|
| `auth_view_body.rs` | 1,060 |
| `auth_manager.rs` (+ its tests) | 1,023 |
| `auth_view_shared_helpers.rs` | 603 |
| `auth_override_warning_body.rs` | 419 |
| `auth_view_modal.rs` | 388 |
| `terminal/view/inline_banner/anonymous_user_ai_sign_up.rs` | 240 |
| the rest of `app/src/auth` | 508 |
| `root_view.rs` | 845 |
| `settings_view/main_page.rs` | 455 |
| `workspace/view.rs` | 361 |

`AuthState` **stays**. It lives in `warp_server_auth` and kept features read it:
session sharing, the cloud object model, the remote-server SSH context and crash
reporting all ask it for a user id or a logged-in flag. It is pinned logged out,
so each of those takes a branch upstream also has. Removing the type would mean
rewriting those callers for no gain.

`AuthOnboardingState` collapsed from seven variants to one. `Auth` (the login
wall), `ConfirmIncomingAuth`, `WebImport`, `NeedsSsoLink`, `Onboarding` and
`PostAuthOnboarding` had no origin left: the fork boots straight into the
terminal and `try_open_onboarding_slides` was already a stub. The single-variant
enum is kept on purpose; collapsing it to a bare `ViewHandle<Workspace>` would
touch ~50 `if let` sites for no behaviour change.

Logging out went with logging in. `maybe_log_out` / `log_out` existed to swap
accounts: they dropped the sqlite database, reset the cloud object model, stopped
the sync and polling loops and left every shared session. With no second account
those were buttons that only wiped local state.

### Four surfaces were live, not dead

Pinning auth logged out in `4683193da` did not hide the logged-out UI, it made it
**unconditional**. Each of these was on screen in the shipped build:

- the workspace header "Sign up" button,
- the settings main page account section, rendering its anonymous branch with a
  "Sign up" button and a "Compare plans" upgrade link,
- an inline terminal banner offering to sign up to unlock AI, in a build with no
  AI,
- the privacy page "Manage your data" section, linking out to delete an account
  that does not exist.

The same inversion cut the other way in `lib.rs`, where a block gated on
`user_is_logged_in` had accumulated work unrelated to accounts: low-power GPU
detection, the graphics-backend dropdown refresh and **crash-recovery frame
tracking**. All three had been silently unreachable. That block now runs
unconditionally.

The lesson generalises: when a predicate is pinned to a constant, every branch on
it becomes unconditional in one direction. Both directions have to be checked,
and "the feature is off" is not the same as "the UI for it is gone".

### A neutralised accessor is not a neutralised value

`should_collect_ai_ugc_telemetry` decides how much of a terminal block gets
serialized: the full output (first and last 2500 lines) when AI-UGC collection is
on, or a truncation to `MAX_SERIALIZED_OUTPUT_LINES` when it is off.

An earlier pass pinned `PrivacySettingsSnapshot`'s accessors to `false`, with a
comment saying telemetry is hard-off in this build. That was true of the
snapshot. It was not true of the value: `terminal_manager` calls the free
function with `PrivacySettings::as_ref(ctx).is_telemetry_enabled`, a *different*
field that still tracks the setting, and that setting defaults to `true`. With
the workspace UGC setting defaulting to `RespectUserSetting` and
`global_ai_analytics_collection` compiled in, the predicate returned true.

So every serialized block was carrying its untruncated output into sqlite, to
feed an upload endpoint that had already been deleted. Nothing failed; the
database just grew.

This is the same shape as the auth surfaces above. Neutralising a predicate in
one place does not neutralise the callers that reach the value by another route.
When pinning something off, grep for the *field* as well as the accessor, and
follow each caller to what it actually controls.

### The rebinding family has a silent member

Removing `WorkspaceAction::LogOut` left its match arm behind:

```rust
LogOut => {
    ctx.dispatch_global_action("app:maybe_log_out", ());
}
```

`LogOut` is no longer a variant, so this is not a pattern match against a
variant. It is an **irrefutable binding** that matches every action, and it sat
near the top of a 500-arm match. Every arm below it became unreachable: opening
the vertical tabs panel, closing a tab, sharing a session, all of it.

This compiled with zero errors. `cargo check` reported it only as
`warning: unreachable pattern`, 180 of them, buried in the 400-plus warnings this
tree already emits. Four configurations were green. The one signal that caught it
was a test asserting a panel opened.

That makes it the worst-behaved member of the rebinding family so far, and it is
the same root cause as the fourteen before it: **deleting an item is not local
when something else names it.** The E0408 or-pattern variant at least errored,
600 lines away. This one produced correct-looking code that silently stopped
dispatching most of the application's actions.

The check is cheap and now belongs beside the parse-error guard on every pass:

    cargo check ... 2>&1 | grep -c 'unreachable pattern'

Expect zero. A non-zero count after deleting an enum variant means an arm was
left behind and is now swallowing its neighbours.

### Files nothing compiles

An orphan sweep (no `mod` declaration, no `#[path]`) found seven files, 1,596
lines, that were not in the binary at all and so were invisible to every error
count. The one that matters: `app/src/server/telemetry/collector.rs`, 229 lines
holding the entire Rudderstack transport, startup flush, 30-second periodic
flush, 60-second active-usage event, shutdown flush. It reads and greps as live
code. `telemetry/mod.rs` had stopped declaring `mod collector`, so none of it
shipped. Deleting on the strength of a module comment would have been luck;
the sweep is the reason to believe it.

That sweep is worth re-running after any large excision: `cargo check` cannot
report on a file it never opens.

## What is left, measured (2026-07-31)

Login, telemetry, the agent, Warp Drive and the cloud settings pages are gone.
What remains is one connected subsystem, not a list of independent targets:

| piece | lines | note |
|---|---|---|
| `app/src/server/cloud_objects` | 12,658 | the sync half |
| `app/src/cloud_object` | 10,393 | model **and** sync; the model backs local objects |
| `crates/graphql` (`warp_graphql`) | 9,855 | 33 app files import it |
| `app/src/workspaces` | 6,062 | teams, plus the Space/Owner mapping |
| `crates/warp_server_client` | 3,554 | 24% login; `iap.rs` serves session sharing |
| `crates/warp_server_auth` | 1,398 | `AuthState` is a kept dependency |

These cannot be taken in any order. `crates/graphql` cannot go before the sync
layer that imports it. `warp_server_auth` cannot shrink without
`warp_server_client`, which the server API layer still needs.

`app/src/workspaces` is the one that looks easiest and is not. It reads as team
and account infrastructure, but 29 of its methods are called from ~30 files
outside it, and about twenty are admin-policy predicates gating **kept**
features: `is_codebase_context_enabled`, `is_voice_enabled`,
`is_prompt_suggestions_toggleable`, `is_enterprise_secret_redaction_enabled`. It
also owns `personal_drive`, `space_to_owner` and `owner_to_space`, the
Space/Owner mapping the local object model uses. This is a carve-out on the shape
of the Warp Drive one, not a deletion.

Every one of those predicates currently returns its no-workspace default, because
`current_workspace()` is `None`. That makes the transformation well defined: each
call site should be replaced by exactly what it evaluates to today. It also makes
it dangerous to eyeball, because getting one wrong silently flips a kept feature
on or off, and 20 of the 29 are booleans where both values look plausible.

**Correction (2026-07-31, same day):** the plan above said to have the accessors
return their no-workspace defaults directly. That is wrong, and the reasoning
behind it was wrong. `current_workspace()` is *not* always `None`.
`UserWorkspaces::new` seeds itself from `cached_workspaces`, read from the sqlite
`workspaces` table, and a database written before the excision can still have rows
there. Anyone upgrading from a build that could log in still has cached workspace
data, so their accessors return real values.

Hardcoding the no-workspace default would silently change their settings, and at
least one of those changes is in the unsafe direction:
`is_enterprise_secret_redaction_enabled` would go from a cached `true` to `false`,
weakening secret redaction for exactly the users most likely to care.

What was done instead: remove only the code that can *change* workspace data, and
leave every accessor reading whatever cache exists. Team administration went that
way (27 methods, 27 events, 516 lines) with no call-site edits at all, because the
Teams settings page and billing modals were already gone and nothing called it.

The general rule this is an instance of: "the predicate is always X today" is a
claim about the *current process*, not about the data. Before folding a predicate
to a constant, check whether persisted state can make it something else on a
machine that is not this one.

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

#### Resolved (2026-07-31): the carve-out, measured

Done in five commits, `9f2d91713..942714bb3`: 161 files, 942 insertions, 12,833
deletions. `app/src/drive` no longer exists. The call for a carve-out was right.
The predicted destinations were not.

`app/src/drive` was 22,179 lines in 46 files. The split:

| | Lines | Files |
|---|---|---|
| Lifted, because it was never Drive | 12,901 | 29 |
| Deleted, because it was the browser | 9,278 | 17 |

**58% of a directory named `drive/` had nothing to do with Warp Drive.** That is
the finding worth carrying forward, and it is the same "location is not
coupling" lesson the agent pass produced, at larger scale. Where it went:

| Lifted to | What it actually was |
|---|---|
| `workflows/arguments_ui/` | the `{{arg}}` prompt modal |
| `sharing/` | the pane-header share button, moved byte-identical |
| `cloud_object/export` + `import` | workflow↔YAML, notebook↔Markdown |
| `cloud_object/folders` | the `CloudModelType` impl behind sync and persistence |
| `cloud_object/styling` | object icon colour, used across eleven files |
| `cloud_object/object_type` | the kind tag every object icon keys on |
| `cloud_object/open_object` | already in the pane restore path |
| `cloud_object/object_limits` | per-plan object quotas |
| `settings_view/team_action_confirmation_dialog` | delete team, leave team |
| `settings/warp_drive` | one session-sharing onboarding flag |

The 9,278 deleted lines are the browser proper: `index.rs` at 5,712 lines was
the single largest file in the directory, plus `panel.rs`, `items/`, the naming
and empty-trash dialogs, and their tests.

Three specifics the prediction got wrong:

- **`env_vars/` received nothing.** `drive/items/env_var_collection.rs` was
  deleted, not lifted: it is the Drive *preview and click action* for an EVC,
  which is browser code. The EVC feature lives in `app/src/env_vars` and needed
  only `cloud_object` and `sharing`, which came out to the top level anyway.
- **`import/` was kept**, though nothing above anticipated it. It is the exact
  inverse of the kept `export`, reads `.yaml` as workflows and `.md` as
  notebooks, and never touched the panel. Shipping one half of a symmetric pair
  would have been arbitrary.
- **`search/command_palette/warp_drive/` survives**, 1,609 lines, and is live.
  Despite the name it is the command palette's searcher over workflows,
  notebooks and env-var collections, reached from `command_palette/data_sources`
  and `terminal/input/prompts`. The naming trap again, in a directory the
  `crate::drive::` sweep never looked at.

Kept deliberately: the `WarpDriveSettings` type name and its `warp_drive` TOML
section. That section name is the persisted path, so renaming it would silently
reset the flag and re-show an onboarding block every existing user has already
dismissed. That needs a migration, not a rename.

One thing the carve-out got wrong, fixed in `05825b77e`: step 1 lifted
`drive/workflows/ai_assist.rs` along with the rest of the argument UI without
reading it. It is the "Autofill" button, which asked Warp's server to generate
workflow metadata. Lifting a file because its neighbours are worth keeping is
the same mistake as deleting one because its neighbours are not.

## The excision compiles (2026-07-30)

`app/src/ai` and its usage sites are gone: 835 files, 292,401 deletions against
`main`. The library and the shipped `warp-nine` binary build clean for
aarch64-apple-darwin with `release_bundle,extern_plist,gui`.

The error count went up before it went down, twice, and both rises were real
progress:

| stage | errors | files |
|---|---|---|
| first trustworthy number | 1,171 | 174 |
| after the name-resolution wave | 1,782 | 17 |
| after the type-checking wave | 414 | 87 |
| after cleanup | 2 | 1 |
| now | 0 | |

While an import is unresolved rustc never type-checks the bodies that use it, so
clearing 1,170 names exposed the usage sites underneath. The signal that the end
was close was not the count but the *mix*: 1,121 `E0433 cannot find` collapsed to
13, replaced by `E0599`/`E0609` and struct-field mismatches. That is rustc
leaving name resolution and entering type checking, which is the last phase.

### Ten rescues

Every one lived under `app/src/ai/` or carried an agent name while having zero
agent dependencies. Path-based and name-based heuristics delete all ten silently.

| rescued | now at |
|---|---|
| `keyboard_navigable_buttons`, `numbered_button` | `ui_components/` |
| `inline_action_header`, `inline_action_icons`, status icons | `ui_components/inline_action/` |
| `toggleable_items` | `ui_components/` |
| LSP / workspace / repo model | `persisted_workspace.rs` |
| `shimmering_warp_loading_text` | `terminal/view/shimmering_loading_text.rs` |
| history autosuggestion lookup | inlined in `terminal/input.rs` |
| `unfreeze_agent_input` | restored in `terminal/input.rs` |
| `ACCEPT_PROMPT_SUGGESTION_KEYBINDING` | `terminal/view/init.rs` |
| `CloudObject`, `UiComponent`, `CompletionContext`, `VoltronFeatureViewMeta` | re-added to over-pruned `use` groups |

`unfreeze_agent_input` is the instructive one. The name says agent; the body only
reads `shared_session_status()` and drives editor state, and three callers in the
shared-session viewer need it. Only reading the body distinguishes it.

The four trait imports are the sneakiest class: a dropped trait is invisible to
grep, because the call site reads `x.method()` and never names the trait.

### Deleting an item without its attribute rebinds the attribute

Two latent bugs, same cause. An attribute whose item is deleted silently attaches
to whatever follows.

- A stray `#[cfg(any(test, feature = "integration_tests"))]` had reattached to
  `handle_task_status_reset`, compiling a live method out of release builds.
- A stray `#[serde(alias = "action_id")]` had reattached to
  `has_agent_written_to_block: bool`. Any legacy row carrying `action_id` (a
  string) fails to deserialize into a bool and silently drops that block's
  metadata. **No compile error** — it surfaces only as a bad database load.

Both were found by hand, so afterwards the tree was swept for each shape. Every
serde `alias`/`rename` across all 255 changed files is byte-identical to `main`,
and no trait import was dropped while its call sites remain.

Note the first sweep had a parser bug: `re.sub(r"\n\s+", " ", src)` collapses
blank lines too, which destroys the `^` anchor for the next `use` statement and
over-reports. Join only `\n[ \t]+`.

### Persistence held

No persisted variant was removed anywhere. Agent-era rows are skipped with a log
line and their siblings restore normally, so a database written by the
pre-excision build still loads. Cloud-object and sync-queue variants are listed
explicitly rather than behind a `_` wildcard, so a new object type is still a
compile error rather than a silent skip.

### Three regions the error lists could not see

Every error list came from `cargo check` on the bundle target, which is blind to:

1. `#[cfg(test)]` sidecars. Nine `*_tests.rs` files are orphaned outright, their
   `#[path] mod` declaration having gone out with a deleted file.
2. The `integration_tests` cargo feature. `app/src/integration_testing` is gated
   on it; five files there still name `crate::ai`.
3. `#[cfg(target_family = "wasm")]`. A wasm build will not compile.

A green `cargo check` on one target is not evidence about the others.

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

| Module | LOC | Status |
|---|---|---|
| `drive` | 22,851 | done: 9,278 deleted, 12,901 lifted |
| `cloud_object` | 6,986 | grew by the lift; see note in the excision order |
| `auth` | 6,799 | |
| `billing` | 492 | |

The `drive` figure here counts 22,851 against the 22,179 measured at the carve-out
commit. The difference is the agent pass, which had already taken ~670 lines out
of `drive/` before the carve-out started.

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
   - [x] `warp_drive_page` — correctly called as **not** a settings-page-sized
         job. Warp Drive was a left-panel subsystem touching `palette.rs`,
         `app_state.rs`, `root_view.rs`, `util/bindings.rs`,
         `auth/login_slide.rs`, `resource_center/mod.rs`, and `app/src/drive`
         (22,179 LOC). Done as its own pass; see the carve-out measurements
         above. The `SettingsSection::WarpDrive` variant stays, because
         `settings_panes.current_page` persists it as a string; only its
         `FromStr` arm is gone, so a saved "Warp Drive" now falls back to the
         default page.
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
     | ~~`warp_drive_page`~~ | ~~282~~ | done |

     Measure the cascade before picking the next one — `ai_page` was the
     largest file but among the cheapest to remove, and `environments_page` is
     the opposite (16 of its 44 sites are in
     `pane_group/pane/environment_management_pane.rs`).
2. [x] Terminal/pane/workspace integration points (410 external `ai::` refs).
3. [x] `app/src/ai` + `crates/ai` + agent crates.
4. Cloud: [x] drive, [ ] `cloud_object`, [ ] graphql. `cloud_object` did not go
   with the Drive browser and should not: it is the object model behind
   workflows, notebooks and env-var collections, all kept, and it absorbed the
   export, import, folders and styling code lifted out of `drive/`. What can
   still go is the sync and server-push half, not the model.
5. [ ] Auth: `app/src/auth`, `warp_server_auth`, `warp_server_client`,
   `firebase`. Note that `AuthState` is already fail-closed by construction:
   the fork removed the secure-storage read, and nothing else sets both a user
   and credentials, so `is_anonymous_or_logged_out()` is permanently true. Every
   `Availability::AI_ENABLED` surface is gated off through that one predicate.
6. [ ] Telemetry + crash reporting.

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

## Tier 1 (2026-08-01)

Sizing a removal by grepping for a module name tells you what to delete, not what
it costs. Four of the seven items surveyed as "tier 1: independent, removable
now, ~4,000 lines" were not independent, and the one that was turned out to be
half as large again as estimated. The survey was wrong in both directions.

### What came out

`crates/managed_secrets` and everything reached through it. Server-stored secrets
via GCP Workload Identity Federation and HPKE envelope encryption, behind
`FeatureFlag::WarpManagedSecrets`. Estimated at 2,332 lines; actually about 3,600
once followed: the crate, `managed_secrets_wasm`, seven orphaned graphql
operations, the `ActorProvider` adapter on `AuthState`, and 269 lines inside
`warp_server_client::iap` implementing an STS token exchange and IAM
`generateIdToken`. That WIF mint had exactly one caller, a sandboxed Oz runner
detected by `OZ_RUN_ID`, which ships without gcloud and so could not use the
ordinary refresh path. `IapManager` keeps the gcloud path it always took here.

Cloud-agent OTLP tracing, 1,248 lines. `tracing::init` built an exporter only
when `WARP_CLOUD_AGENT_OTLP_ENDPOINT` was set and installed a no-op subscriber
otherwise, so the early return was the only reachable path in this fork.

`server_api::download` and `server_api::integrations`, orphans. The referral
fetch path and the reward modal it drove, 572 lines.

### Two more accessor-vs-value findings

The family now has five members. Both of these were found the same way: by asking
what a removed predicate had been gating, rather than by reading the removed code.

`ServerVoiceTranscriber` had already been reduced to a `transcribe` that always
returns an error, because the endpoint went with the agent. But it was still
registered, and `VoiceTranscriber::transcriber()` returning `Some` is precisely
what `editor/view/voice.rs` reads to decide whether voice input is available. So
the editor offered it, recorded the audio, ran the session to completion, and
only then surfaced a failure, once per attempt. `None` is the disabled state the
type already models. Neutralising an implementation is not neutralising what
callers read to reach it.

`ReferralThemeStatus` was nearly a bug I introduced rather than found. It looked
like an obvious full delete with the two reward themes hard-coded hidden, since
nothing can unlock them without a server. But `new()` reads `ReferralThemeActive`
and `ReceivedReferralTheme` out of *persisted user preferences*, so anyone who
earned a referral theme before this fork existed still has it recorded, and
`theme_chooser` still offers it. Deleting the model would have quietly taken away
a theme they had unlocked. The model is kept as a read-only view and only the
fetch is gone. Same shape as `current_workspace()`: "always false today" was a
claim about the fetch, not about the stored data behind it.

### What was not tier 1, and why it is cheaper later

- `server_api::auth` -- `UserAuthenticationError` feeds `sync_queue` and the
  shared-session viewer, both tier 2. Genuinely blocked, not merely awkward.
- `network_log_view` -- a full pane type wired into the pane enum, launch
  configs, the settings view and vertical tabs. Trivially dead once the network
  layer goes; unwinding a pane enum before then is strictly worse.
- `server/experiments` -- fed by `server_api`, cached in sqlite (so a
  pre-excision database has real rows), and consumed by `user_workspaces`.
- `server_api::block` -- listed as a 163-line leaf. It pulls a 1,489-line
  `share_block_modal` across eight files including the terminal action enum and
  the `show_blocks_view` settings page. A feature removal, not a leaf.
- `server_api::team` -- 108 lines, but `MockTeamClient` is threaded through seven
  or more test files.

`retry_strategies` and `ids::ServerId` remain load-bearing for the local object
store, as recorded earlier.

## Tooling: a helper that fails silently is worse than no helper

`cutfn.py` is a library with no `__main__` block. Invoking it as a script ran the
imports, defined the function, exited 0 and changed nothing. Three "successful"
cuts later the functions were all still in the file. Nothing in the exit status
said so.

This is the same class as the attribute-rebinding and or-pattern traps that this
file already records, and it is the reason the count of those stands at sixteen
with three caused by tooling rather than found by it: the failure is silent and
the signal that should have caught it reports success. Every cut since goes
through a driver that re-reads the file afterwards and asserts the target string
is gone. `cut.py` already did this, which is why it caught the `code_page.rs`
ambiguity; `cutfn.py` did not.

The general rule for this project: a deletion helper must verify its own
postcondition, because the compiler cannot distinguish "nothing to delete" from
"deleted nothing".

## Signing and notarising this fork (2026-08-01)

`script/macos/bundle --developer-id` signs with a Developer ID Application
identity from the login keychain, using the production entitlements, hardened
runtime and a secure timestamp. `--notarize --notary-profile <name>` submits and
staples. Upstream's `--codesign` path is unusable here: it imports a base64 .p12
from `WARP_DEVELOPER_ID_CERT` into a throwaway keychain and signs against
upstream's hardcoded team. `--selfsign` cannot substitute either, because it
signs with `Debug-Entitlements.plist`, whose `com.apple.security.get-task-allow`
is rejected outright by the notary service.

`Entitlements.plist` requested app group `2BBY89MBSN.dev.warp`, upstream's team.
An app group must be prefixed with the signing certificate's own team ID, and on
Developer ID it also needs an embedded provisioning profile. Removing it is
behaviour-neutral: `app_group_container_path` probes the container by writing a
temp file and returns `None` when that fails, so `secure_state_dir()` already
returned `None` and all four callers fall back to `state_dir()`. Anyone re-adding
it under a new team should know that turning the app group on relocates the
sqlite database into the group container, so existing installs come up with no
history.

Three packaging defects, each found by inspecting an artifact rather than by
reading code:

A missing `create-dmg` aborted the run under `set -e` after a completed build and
a valid signature. The Finder AppleScript inside `create-dmg` dies with
`AppleEvent timed out (-1712)` on any machine whose controlling process lacks
Automation permission for Finder, which is the normal state for a build started
from a terminal; upstream's `--skip-jenkins` escape hatch was scoped to one CI
runner. The image was assembled from `$BUNDLE_DIR`, which also contains the
`dmg/` working directory, so it shipped a copy of that folder next to the app.

The ordering one is the subtle one. Stapling writes the ticket *into* the bundle,
so an image built from an unstapled app contains an unstapled app however
thoroughly the image itself is stapled afterwards. Notarisation therefore runs
before packaging. That in turn made any notarisation failure fatal to the whole
run, and since the script clears the previous build's artifacts on startup, a
credential that stopped resolving destroyed the prior notarised output too.
Failure is now recorded, packaging still runs, and the script exits non-zero.

## Tier 2: the cloud object write path (2026-08-01)

Three commits, 20,473 lines. This is the stack that sat behind the thirteen
`UpdateManager` methods left after tier 1.

`UpdateManager` was the junction between three parties: the views that mutate
workflows, notebooks, folders and env var collections; the sync queue that carried
those mutations to the backend; and the real-time channel that carried other
clients' mutations back. Both server-facing sides are gone, and every write now
lands in the in-memory model and in sqlite, synchronously.

### Five dead affordances, all the same shape

Each of these methods opened with a guard that returned early when the object had
no server ID, and an object created in this build never gets one. The guard was
invisible: no error, no log line, nothing on screen. The affordance was enabled
and did nothing.

- Trash, in the workflow, notebook, env-var-collection and workflow-argument
  menus. There was no way to remove a workflow you had made.
- Untrash, so anything trashed before this fork stayed trashed.
- Permanent delete.
- The notebook edit baton. The common call hid it -- the caller switches to edit
  mode itself -- but `grab_edit_access(false, ..)` is what the "someone else is
  editing" modal's Take Access button dispatches, and that caller waits for the
  server's answer before switching. Nothing creates that state locally, but a
  notebook synced before this fork can still carry another user's uid in its
  persisted metadata, and then the modal is reachable.
- Drive sharing. `ShareableObject::WarpDriveObject` carries a `ServerId`, so it
  could not even be constructed for a local object; the Share button never
  appeared. Session sharing is a different feature on a different transport and
  is untouched.

The first four are local operations now. The fifth is gone.

### Three bugs the removal would have introduced

Removing a response handler is not free when something else was counting on it to
run.

`GenericCloudObject::new_local` started every object at `InFlight(1)`, counting the
create request that used to follow. Nothing decrements it any more, so every object
a user created would have been permanently "unsaved" -- which is what
`num_unsaved_objects_to_warn_about_before_quitting` counts. Quitting after making a
single workflow would have warned about unsaved work already on disk, every time,
with no way to clear it. `update_object` incremented the same counter on every edit.

Env var collections mark themselves `Unsaved` while editing and were marked `Saved`
again by the server's answer. `should_disable_invoke` keeps the Load button disabled
unless the status is `Saved`, so the button would have gone dead at the first
keystroke and stayed dead.

The general lesson: when you delete the half of a pair that clears a flag, find
every reader of that flag before you delete the half that sets it.

### Rebuilding rather than deleting

The Warp Drive import queue existed because a file inside a folder could not be
uploaded until the folder came back from the server with an id. A child points at
its parent's client id now, so the import runs straight through; folder import is
no longer gated on the queue running, and the "still syncing" spinner threaded
through three call layers became the "saved locally" icon it always resolved to.

`UserWorkspaces` and `Workspace` were kept when their fetch went, for the same
reason the referral theme model was: they load from sqlite, so a user who belonged
to a team before this fork still has that workspace and its settings. Deleting the
model would have taken away data people already have.

### Orphan detection: match the import path, not the name

Thirty-nine GraphQL operations had no consumer. Name matching reported 27 of them
as live, every one a false positive: `create_folder` matched
`UpdateManager::create_folder`, `MoveObject` matched a comment, `TrashObject`
matched an unrelated workflow-modal action, and the generated `User`, `Workspace`,
`Block`, `Task` and `Space` types matched something in almost every file in the
tree. A cynic operation is only reachable through `warp_graphql::mutations::<name>`
or its grouped form, so that path is the only sound thing to search for.

### Tooling: `cargo fix` is not usable here, and neither was my replacement

`cargo fix --lib` strips imports that only `#[cfg(test)]` code reads. It did:
`crate::auth::{credentials, user}`, `InitialLoadResponse`, `ServerIdAndType`, and
`Arc`, `UserUid` and `MembershipRole` in `user_workspaces`. `cargo fix
--all-targets` applies the lib fixes first and then fails on the tests it just
broke. Both passes were reverted.

The replacement read warnings from the all-targets build instead, which is the
right input, and still went wrong: two warnings on the same line each deleted a
line, so the second deletion took the neighbouring import with it, and in one case
a closing brace inside a `cfg_if!`. Six files were damaged, one of them into a
parse error. Everything was restored by hand and the sweep abandoned. Some
unused-import warnings remain. A warning is not worth a broken build.

The three re-exports that only tests read now carry `#[allow(unused_imports)]` and
a comment saying so, to stop the next person removing them.

### What is not behind the UpdateManager methods

`warp_server_client` (3,022 lines) and `warp_server_auth` (1,391) survive. They
were named as part of this tier, but they are not gated on the cloud-object path:
they back shared terminal sessions, the remote-server SSH context, API key
management, the multi-agent client and crash reporting. Removing them means
removing session sharing, which is a separate decision.

(Tier 3 made that decision and removed session sharing. Both crates still stay:
`remote_server`, API keys, the multi-agent client and crash reporting were always
the larger share of their callers.)

`server_api` is reduced but not gone. What remains is the auth client (API keys,
token refresh) and the HTTP client that session sharing and project init use.

## Tier 3: shared sessions (2026-08-02)

26,715 lines removed, 606 added, across 141 files. 19,584 of those lines were the
`shared_session` and `sharing` trees themselves; the rest was call-path code
threaded through the terminal view, the input, the pane group and the workspace.

### What it was

A real-time collaborative terminal, not a link to a recording. The sharer opened a
WebSocket to `wss://sessions.app.warp.dev` (hardcoded in `WarpServerConfig::production`)
and streamed scrollback on join, PTY reads batched at 50ms, ordered terminal
events, text selections throttled at 20ms, participant presence and window size.
Viewers joined through `warp://shared_session/{id}` and, depending on role, could
type. Roles were `Reader`, `Executor`, `Full`; access came by link, by invited
guest, or by team ACL. The input line was a CRDT, so participants co-edited the
prompt rather than watching a mirror.

Server-relayed, not peer-to-peer. The wire format lived in a separate git
dependency, `warpdotdev/session-sharing-protocol` pinned at `b30fdd06`, which is
now gone from `Cargo.toml` along with its `[patch]` stanza.

### The half that was already dead, and the half that was not

Creating a shared session was unreachable. `open_share_session_modal` had an
`is_anonymous_or_logged_out()` early return added when sign-in was closed off, and
`creating_shared_sessions` was not in the default feature set, so
`FeatureFlag::CreatingSharedSessions` was off in every build this fork produces.

Joining one was not. `viewing_shared_sessions` was on by default, the URI host
parsed, and the join payload sent `anonymous_id` with `access_token: None`. Whether
the server accepted an anonymous joiner is a server-side question that cannot be
answered from this tree. Either way, an outbound WebSocket to Warp's
infrastructure is exactly what this fork exists to not have.

That asymmetry is why this needed a decision rather than a sweep, and it corrects
an earlier claim in these notes that shared sessions were "roughly 13,000 lines of
live terminal functionality."

### What came out with it

- `app/src/terminal/shared_session/` (13,107 lines) and
  `app/src/terminal/view/shared_session/` (3,073): the sharer and viewer network
  layers, the presence manager, the permissions manager, the role-change modal,
  the share modal, the participant avatars.
- `app/src/sharing/dialog/` (2,442) plus its QR code renderer and style module.
  `app/src/sharing/mod.rs` survives as a re-export shim for `ContentEditability`
  and the `cloud_objects` ACL types, which the notebook, workflow and env-var views
  still speak.
- `app/src/pane_group/pane/view/header/sharing.rs`, the pane header's share button
  and view-only indicator, and the `HeaderRenderContext::sharing_controls` closure
  that fed it.
- Five feature flags: `CreatingSharedSessions`, `ViewingSharedSessions`,
  `SharedSessionWriteToLongRunningCommands`, `SessionSharingAcls`,
  `AgentSharedSessions`.
- `UserKind::SharedSessionParticipant` and `TeamKind::SharedSessionTeam` in
  `cloud_objects`, the last things in that crate that needed the protocol.

### What the model layer lost, and what that changes

`TerminalModel` held `shared_session_status`, `shared_session_source`, and two
outbound channels. Removing them collapsed a set of conditionals that are worth
recording, because each one was a behaviour difference that no longer exists:

- Resize no longer takes the max of the local size and the sharer's, and no longer
  suppresses reflow. Every remaining resize is a genuine local pane or font change.
- The alt screen and the blocklist are no longer horizontally scrollable. Both were
  scrollable only for a viewer whose pane was narrower than the sharer's terminal.
- Secret obfuscation is no longer force-disabled. Sharing turned it off for the
  whole session; a local session honours the setting.
- `should_validate_dcs_hook_session_id` is unconditionally true. Only a viewer
  skipped it, because the session ids in a replayed scrollback belong to someone
  else's shell.
- Closing a tab no longer prompts. The only thing that made a tab require
  confirmation was an active share in one of its panes, so
  `should_confirm_close_session` now reduces to the user's setting alone.

### Three tests that were nearly lost

`cut_tests` matched on any mention of `shared_session` and took
`normal_lifecycle_pipeline_emits_completion_and_prompt_side_effects_once` and two
siblings with it. They are lifecycle tests: they assert that a command's completion
and prompt side effects fire exactly once. They mentioned shared sessions only
because they used the ordered-event channel as a second observation point. All
three were restored with that one assertion dropped and a comment explaining the
gap. A needle that matches the observation mechanism is not a needle that matches
the subject.

### The unused-import sweep, second attempt

The first attempt at this (see the tier-2 notes) corrupted six files. This one used
the compiler's own JSON spans rather than line numbers, blanked exactly the flagged
columns, and then ran a second pass to repair the resulting `use X::{, };`. That
second pass was itself the problem: a regex over `^use [^;]*;` flattened multi-line
statements onto one line, and a `//` comment inside one of them commented out the
rest of the import list. One file broke that way and was repaired from `HEAD`.

Two imports were removed that the lib genuinely did not use but the tests did
(`AuthStateProvider`, `ShellName`, and the `Subject`/`UserKind` re-exports). This
is the same trap as `cargo fix --lib`, arrived at by a different route: a
lib-only unused-import list is not the same as an unused-import list.

The lesson stands from last time and now has a second data point. Repairing
malformed Rust with regex produces new malformed Rust. If a sweep needs a repair
pass, the sweep was wrong.

### The endpoint that survived the excision

The tier-3 pass removed every caller of the session sharing server and then
declared itself done. It was not. `WarpServerConfig::production` still carried

```rust
session_sharing_server_url: Some("wss://sessions.app.warp.dev".into()),
```

and `strings` on the signed binary found it. A fork whose entire premise is not
talking to someone else's servers had that server's address compiled into the
shipped artifact.

Nothing read it. `ChannelState::session_sharing_server_url()` had no callers, the
`--session-sharing-server-url` flag had no consumer, and
`WITH_LOCAL_SESSION_SHARING_SERVER` pointed at a build step that no longer
produced anything. It was inert, and it was still there, because "no callers"
was checked in Rust and the constant is data.

Removed with it: the CLI flag and its `WARP_SESSION_SHARING_SERVER_URL` env
override, the `rerun-if-env-changed` line in `app/build.rs`, the feature-to-env
mapping in `script/run` and `script/wasm/bundle`, two `.vscode` tasks that
invoked a feature that no longer exists, and the `.github/STAKEHOLDERS` entry
for a deleted directory.

The lesson generalises past this fork. Checking that a subsystem has no remaining
callers proves the code is unreachable, not that its configuration is gone.
Endpoints, keys and bucket names live in constants that compile in whether or not
anything reads them. `grep` the built artifact, not just the source.
