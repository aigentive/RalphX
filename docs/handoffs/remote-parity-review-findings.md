# Adversarial review findings — remote parity branch

Five independent adversarial reviews of `feat/remote-multi-env` (base `origin/main`), each with a
different lens: correctness/state, host-alone parity, dishonest UI, test quality, and cleanup.

**Reading this report:** ✅ VERIFIED means I re-checked the claim against the code myself and it
holds. Unmarked findings are the reviewer's, plausible but not independently confirmed — treat them
as leads, not facts. Nothing here has been fixed; this is the ledger.

---

## Tier 0 — fix before merge

### 1. The plan-edit "lost-update guard" does not guard the write ✅ VERIFIED
`src-tauri/src/application/plan_artifact_edit.rs:273-335`, `application/startup_background.rs:1638`

`expected_version` is enforced in exactly two places, both *reads*, both in their own `db.run`
before the write: persist time (`remote_plan_edit_intent.rs:78`) and claim time
(`startup_background.rs:1638`). `update_plan_artifact_for_state` takes no version parameter — its
transaction re-resolves `resolve_latest_sync` and writes unconditionally.

So the CAS is check-then-act across an await boundary. Artifact at v5 → remote client requests an
edit against v5 → dispatcher claims and the check passes → during the multi-await window (session
reads, `check_verification_freeze`) a local user saves → v6 → the dispatcher's transaction resolves
latest = v6 and writes v7 carrying content authored against v5. **The v6 edit is silently lost and
`REMOTE_PLAN_EDIT_VERSION_CONFLICT` never fires.** The commit that introduced this is named
"plan-artifact edit intent twin with lost-update guard".

**Fix:** compare `old.metadata.version` against the intent's `expected_version` *inside* the
`run_transaction` closure, where writes serialize.

### 2. A local read now fails when a remote-only cache write fails ✅ VERIFIED
`src-tauri/src/commands/mcp_policy_commands.rs:445`

`build_catalog` now *ends* with `persist_catalog_snapshot(...).await`, which `?`-propagates both
`serde_json::to_string` and each per-provider `upsert`. Those errors surface from the **local**
`get_mcp_catalog` and `refresh_mcp_catalog`. A host running alone with a transient DB problem now
gets an error where it previously got its catalog — and the snapshot's only reader is the remote
twin, so failing the local read buys nothing.

The correct pattern is next door: `persist_repository_capability`
(`commands/project_commands.rs:178`) returns `()` and swallows the same class of failure with
`tracing::warn!`. **Fix:** make the MCP snapshot write non-fatal the same way.

### 3. Gated ticket actions render fully enabled and silently do nothing
`frontend/src/components/ticketing/TicketDetailSheet.tsx:261-277, 385, 412, 622, 644`

The dashboard closes the gate by passing `onTransitionTicket={gate.gated ? undefined : handler}`,
but the sheet computes `statusDisabledReason` / `assignDisabledReason` / `commentDisabledReason`
from provider **capabilities only** — it never checks whether the handler arrived. On a gated remote
environment the Status select, "Assign to me", "Clear assignee" and "Add comment" look live; the
click runs `void onAssignToMe?.()` and nothing happens. No error, no explanation. Worse than a dead
control, because the user is told the action is available.

### 4. Enter bypasses the send gate, and the gate check is a stale closure ✅ VERIFIED
`frontend/src/components/Chat/ChatInput.tsx:246, 270-280`

`handleSend` correctly guards on `agentGate.gated` — but `agentGate.gated` is **absent from the
`useCallback` dependency array** (`[value, isSending, isAgentAlive, onSend, isControlled,
onChangeProp, questionMode]`). The memoized handler therefore keeps a stale gate value across an
environment switch. This is a real defect independent of any test.

The test that claims to cover this (`agent-gate-surfaces.test.tsx:110`) asserts `toBeDisabled()`
then `.click()`s a disabled button — a jsdom no-op. No test presses **Enter** under a remote
environment without `ui:agent`. That file's own header warns against exactly this mistake.

---

## Tier 1 — security-relevant test theatre

These tests are the *sole* protection for the behaviour they name, and each has a verified mutation
that keeps them green.

| # | Test | Mutation that stays green | Consequence |
|---|---|---|---|
| 5 ✅ | `scope_suite_tests.rs:407` pinned-permission-op | Test calls `registry::extract_pinned_arg` **directly**, never `dispatch`. Change the macro arm `(pinned_arg args:)` → `(arg args:)` at `registry.rs:1079` | A `ui:operate` device approves permission requests via `deny_permission_request`. Its docstring claims it "cannot pass while the dispatch path uses something else" — it can |
| 6 | `ws_tests.rs:835` revoke-before-upgrade | Text-scan only. Drop the `device.revoked_at.is_none()` guard at `ws.rs:846` | Revoked devices upgrade. `ws_events_handler` has **zero** behavioural coverage repo-wide |
| 7 | `ws_tests.rs:891` session-guard-before-upgrade | Stop the closure owning the guard | A failed upgrade permanently consumes one of the device's 8 cap slots |
| 8 | `ws_tests.rs:422` epoch teardown | Make `close_with_teardown` a no-op for `EpochChanged` | A live-looking socket that drops every durable event |
| 9 | `remote_host_commands_tests.rs:194` stream install | Wrap the install in a never-true condition | First enable leaves the event stream absent process-wide; every WS subscribe 503s until restart |
| 10 | `startup_remote_resume_tests.rs:156` stale claims | The test calls `fail_stale` **itself**; never invokes the dispatcher. Delete the sweep at `startup_background.rs:1497` | Stale leases never swept at startup |

**Cross-cutting:** 14 Rust test files rely on `include_str!` source-text scanning. Two recurring
mechanical flaws: `.split(signature).nth(1)` takes *the rest of the file*, not the function body
(`ws_tests.rs:397`, `invoke_tests.rs:1326`), and every such assertion survives replacing the named
call with a no-op or a same-named helper.

---

## Tier 2 — correctness and parity

| # | Finding | Where |
|---|---|---|
| 11 | Optimistic rollback stomps the unknown-outcome reconcile. TanStack awaits the **cache** `onError` before the mutation's, so invalidate-then-rollback ends at the pre-mutation state, freshly timestamped. Only `useIdeationSettings` lacks a settling invalidation; every other rollback mutation self-heals via `onSettled` | `frontend/src/hooks/useIdeationSettings.ts:54` |
| 12 | Queued send-now can kill the wrong run. `ManualNow` stops whatever agent is running; `expected_active_run_id` is **optional** and checked at claim, not inside the stop. Also a second client-reachable path to `pkill`, contradicting the invariant documented at `startup_background.rs:1150` | `startup_background.rs:1376` |
| 13 ✅ | `get_agent_running_states` went fail-open → fail-closed: main returned `HashMap` (idle default on registry failure), ours returns `Result`. Local task-list/status commands now error where they used to render "everything idle" | `application/chat_service/mod.rs:8643` |
| 14 | Post-claim `?`-propagation wedges intents in `starting` until restart, since `fail_stale` runs only at boot. For plan edit this bricks *all* later remote edits for that artifact (`find_unsettled_for_artifact` treats `starting` as unsettled) | `startup_background.rs:1630` |
| 15 | Recovery-prompt dedupe became process-global inside an otherwise verbatim extraction (`.with_prompt_tracker(Arc::clone(...))` added). Command-built reconcilers now share the background runner's markers, changing local re-prompt behaviour both ways | `application/execution_recovery.rs:43` |
| 16 | Delegated-tool snapshot reads flipped fail-open → fail-closed inside an extraction (marked deliberate, still a local parity break) | `unified_chat_commands/mod.rs:2593, 2696` |
| 17 | Local review-TTL expiry (2s) deletes the 24h **remote** snapshots, and the summary snapshot is not re-stored on that path — so advertised client snapshot availability is really bounded by host UI activity | `commands/diff_commands.rs:781` |
| 18 | Five remote-intent dispatchers poll SQLite every 1–2s on purely local hosts. Event capture and the listener are gated on remote settings; only the dispatchers are not | `application/startup_pipeline.rs:236` |

---

## Tier 3 — dishonest UI

| # | Finding | Where |
|---|---|---|
| 19 ✅ | `changedFileCount = reviewQuery.isSuccess ? changes.length : null` — a null snapshot ("host never captured") still reports `isSuccess`, so the Changes tab wears an authoritative **"0"**. Acted upon: the same `[]` picks the review dialog's landing tab. The sibling `hasNoDetectedChanges` at `:930` *is* correctly guarded with `data != null`, so the distinction was known and these derivations were missed | `AgentsPublishPanel.tsx:1565, 2360` |
| 20 | The "not captured" notice is unreachable on two live paths (`snapshotUnavailable` needs *both* queries, but the summary query is disabled for terminal/blocked workspaces), so a merged remote workspace renders "No workspace changes detected" from an unknown | `AgentsPublishPanel.tsx:935` |
| 21 | ~30 controls hard-`disabled` with an unreachable reason, violating the repo's own soft-disable rule (shadcn `Button` carries `disabled:pointer-events-none`). Worst: four plan-lifecycle actions omit `disabledReason` while their **siblings in the same file** set it; `StatusManagementDialog` shows "Syncing statuses" when the real cause is a closed gate | `AgentsArtifactPane.tsx:3851+`, `AgentsActiveConversationPanel.tsx`, `ExecutionControlBar.tsx:719`, automations family, `TicketingDashboardView.tsx` |
| 22 | Changes tray tab **disappears** on a null snapshot, indistinguishable from "workspace is clean" | `AgentsComposerWorkspaceChangesCard.tsx:928` |
| 23 | "Conflicted: 0" rendered while the query is loading or errored, while sibling badges honestly say "Loading" | `AgentsPublishRepairState.tsx:53` |
| 24 | New `notInspected` capability falls through to "could not inspect" when the truth is "has not inspected yet"; the PR-mode toggle renders unchecked, asserting "off" | `types/project.ts:139`, `RepositorySettingsSection.tsx:342` |

**Structural gap:** `agent-gate-guard-manifest.ts` only certifies that a file *imports* the gate, and
`agent-gate-surfaces.test.tsx` asserts reason-reachability for **context-menu items only**. Nothing
guards it for buttons — which is why the whole of #21 is invisible to the ratchet.

---

## Tier 4 — cleanup (low risk, quick wins)

| # | Item | Size / risk |
|---|---|---|
| 25 ✅ | `LOCAL_ONLY_COMMAND_NAMES` is dead — exactly one reference repo-wide (its own definition). Its doc comment claims it feeds the P-11 drift scan; the scan actually reads the file by path and parses source text | 1 file, ~9 lines. Zero risk |
| 26 ✅ | `is_local_proposal` duplicated: `application/ideation_finalize_execution.rs:24` re-implements `commands/ideation_commands/mod.rs:8` (same logic, only local variable names differ). Collapse toward the application layer and re-export | 2 files, ~12 lines. Zero risk |
| 27 | The soft-disable prop triple is hand-rolled at 10 button sites across 5 files while menu items use the shared helper. Add `explainedDisabledButtonProps(gated)` beside `EXPLAINED_DISABLED_MENU_ITEM_CLASS`, keeping emitted attributes byte-identical (tests query `data-disabled-explained`) | 6 files, net ~-20 lines. Near-zero |
| 28 | Minified Rust in 8 `sqlite_remote_*_request_repo.rs` files (single lines up to 663 chars) plus 2 migrations — rustfmt bails on the whole expression because the embedded SQL can't fit. Same class as the `plan_artifact_edit.rs` line that hid a clippy error behind it and masked 12 more | 10 files, ~25 lines re-wrapped. Zero if SQL text is kept byte-identical |
| 29 | Shared test helper for remote-env setup: an identical `afterEach` reset trio in 9+ files, an identical TooltipProvider `render` wrapper in 5, and copy-pasted store seeding in 12. An equivalent already exists, buried at `detail-views/agent-gate.test-utils.tsx` | New ~60-line file; full adoption ~15 files. Test-only |

---

## Gaps worth naming

- **`DEFAULT_PAIRING_BRAKES` names two ops that do not exist** (`executionPause`/`executionStop`; the
  real keys are `executionPlanPause`/`executionPlanStop`). The loop does `if (op === undefined)
  continue`, so the invariant is silently checked over 3 of 5 named brakes. Tests are not in
  tsconfig, so it never typechecked. A latent false **green**.
- **No production mutation declares `unknownOutcomeQueryKeys`**, so every unknown outcome invalidates
  the whole client — awaited *before* the mutation reaches its error state, delaying the caller's
  error UI while every active query refetches over the same degraded transport.
- **`verifyAuthorityNow` has no real-implementation test**: one production caller, one fake, and the
  only test asserting its behaviour mocks the module that contains it.
- **`supervisor.test.ts` is a 126-case tautology** — it asserts the transition table against itself
  and never asserts `row.effects`.
- **The `KNOWN_GATE_GAPS` quarantine is now empty**, so `it.each(expectedUngated)` registers zero
  tests. Harmless in itself, but the manifest prose (`agent-gate-guard-manifest.ts:57-70`) still
  describes five quarantined entries and is now stale.
- **Registration proofs that `include_str!` `registry.rs`** (`remote_environment_commands_tests.rs:166`)
  stay green if a `#[cfg(debug_assertions)]` line is added above an entry — an attribute already in
  use at `registry.rs:22`.

## Good news, confirmed

The invented `{outcome:"error"}` envelope this effort kept finding is **fixed**: 18 uses of
`commandError` and 24 of `outcome:"ok"` across the remote API tests, with
`chat-remote-conversation-lifecycle.test.ts:140` carrying a comment pinning the correct shape. No
test on the branch mocks its own module under test.

## Confirmed sound (do not re-audit)

Claim/terminal CAS and authority-after-claim ordering across all dispatchers; pre-allocated ids are
genuinely consumed by their seams; `/invoke` dedup RAII is owned by the spawned continuation; the
`plan_artifact_edit.rs` reformat is statement-for-statement identical; `queryClient.test.ts`'s
production-path cases; `scope_suite_tests.rs` dispatch-level tests with anti-vacuity counters; the
gating layer provably cannot affect a local host (`resolveAgentGate` short-circuits `!isRemote`, the
store always re-prepends `LOCAL_ENTRY`); pre-existing local TTLs (2s review, 30s PR annotations) are
untouched; Wave C twins never populate and local paths never read snapshots.
