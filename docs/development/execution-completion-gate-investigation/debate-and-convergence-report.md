# Debate & Convergence Report — Execution Completion Gate Branch

**Branch:** `fix/execution-completion-gate-validation-cache` (merge-base `342a35d9f`, post fix commits `986369212`, `32439e8c1`, `fa5997ae2`, `98490627b`)
**Date:** 2026-06-26
**Scope of this report:** consolidates the two multi-agent workflows run after the original 20-agent investigation — (1) verify-findings + design-debate + adversarial convergence, (2) independent confirmation of the load-bearing design decision + finalization. It is the narrative/audit companion to the deliverables (`implementation-spec.md`, `tracker.md`, `report.md`).

> **No production code was changed by any of this work.** All outputs are specs/reports under `.artifacts/specs/current-branch-investigation/`.

---

## 1. Executive Summary

**Verdict: DO NOT MERGE AS-IS.** The branch correctly fixes a real *false-negative* (a completed worker with green HEAD-matched validation evidence was marked `failed` because a lingering terminal `failed` step trapped it). To do so it added green-cache "rescue" paths and zero-step output completion — and in the process opened several *false-positive* completion paths.

- **11 findings examined; 10 confirmed still-real on current HEAD; 0 already fixed** by the branch's 4 commits. (The 11th, F7 sub-claim 2 / Codex delegation, was dismissed as NOT-A-BUG.)
- **4 findings block merge** (F2, F1b, F3, F4) — they share one root: the completion seam reasons about *time and artifacts* instead of *which run is the live attempt*.
- The debate **corrected its own first answer**: its initial design contained a CRITICAL flaw (an unconditional cache clear that re-introduces the very false-negative the branch fixes). The adversarial loop caught it and pivoted to a cleaner mechanism.
- The final design — an **episode-freshness cache gate** — **survived an independent adversarial refutation round** with zero blocking gaps.

**Final convergence status: CONVERGED for implementation handoff.** The merge-blocking architecture is independently confirmed sound; the one non-blocking spec-completeness gap found in the final round (F5b path-containment) was corrected in the spec.

### Cost / scale
| Workflow | Agents | Output tokens | Tool calls | Wall-clock |
|---|---|---|---|---|
| WF-1 verify + debate + converge | 23 | ~3.06M | 344 | ~55 min |
| WF-2 independent confirmation + finalize | 3 | ~0.43M | 63 | ~14 min |
| **Total** | **26** | **~3.49M** | **407** | **~69 min** |

---

## 2. Method

Three stages, each adversarial by construction:

**Stage 0 — Original investigation (prior turn, 20 read-only subagents).** Reviewed disjoint slices of the branch, aggregated a debate, and produced `report.md` + `tracker.md`. Verdict: do-not-merge, 7 headline blockers. This report does not re-derive Stage 0; it is the input the workflows re-verified.

**Stage 1 — WF-1 (Verify → Design → Converge → Handoff).**
- **Verify:** 11 agents re-opened the cited code on *current HEAD* and judged each finding still-real vs. already-fixed (the branch had 4 fix commits, so the report could be stale). Structured output per finding.
- **Design:** 3 architects designed competing end-to-end fix architectures through distinct lenses (minimal-surgical / run-identity-first / fail-closed); an adversarial judge ranked and grafted them into one architecture.
- **Converge:** draft spec → dual-lens adversarial critics (round 1) → revise → critics (round 2) → revise. Each round attacks the *revised* plan.
- **Handoff:** wrote `implementation-spec.md`.

**Stage 2 — WF-2 (Confirm → Finalize).** WF-1 reported non-convergence (round-2 critics still found blocking gaps that the final revision *claimed* to resolve but which had no confirming pass). WF-2 closed that loop:
- An **independent skeptic** (which had not designed the pivot) was told to *refute* the load-bearing decision against live code.
- A **fresh F5b grounding** agent re-ran the one Verify agent that had crashed.
- A **finalizer** applied corrections and updated the spec/tracker.

---

## 3. Stage 1 — Findings Verification (against current HEAD)

| F-id | Still real? | Severity | Blocks merge | Verified current behavior (1-line) |
|------|-------------|----------|--------------|-------------------------------------|
| **F1** stale-attempt guard | YES (pre-existing on main) | HIGH | No (on its own) | Both finalizer guards compare run start time against the **earliest** status entry (`get_status_entered_at`: sqlite `ORDER BY created_at ASC`, memory `.min()`); re-enterable statuses let a stale first-attempt run pass. Also fails **open** on DB `Err`. |
| **F1b** identity-unknown cache bypass | YES (surfaced during debate) | HIGH | **YES** | `validated_completion_override` is repo-free (reads `task.metadata` + `get_head_sha`); under an `IdentityUnknown` flow a stale finalizer could self-rescue via a HEAD-matched cache, bypassing the step gate. |
| **F2** validation-cache scoping | YES | HIGH | **YES** | Cache is task-level, HEAD-only, never cleared on re-entry, no attempt id; a prior attempt's green cache rescues a later attempt at unchanged HEAD. **Two** un-scoped readers (completion finalizers + shared get_task_context hint). |
| **F3** step-repo fail-open | YES (branch regression) | MEDIUM | **YES** | `has_tracked_steps()` collapses `Err` → `false`, conflating "no steps" with "DB failed"; a transient error routes incomplete-step work to `PendingReview` via `else has_output`. Merge-base failed **closed**. |
| **F4** zero-step no-execution_complete | YES (branch broadened) | MEDIUM (promoted) | **YES** | Zero-step advances on `has_output` alone; the in-stream `CompletionSignalTracker.was_called()` is dropped at the `StreamOutcome` boundary. Plumbing touches all 3 `handle_stream_success` call sites. |
| **F5a** setup-failure not blocking | YES | MEDIUM | No | `run_pre_execution_setup` discards `_setup_had_failures`; readiness reflects only install failures, so a hard `worktree_setup` failure doesn't block spawn. |
| **F5b** path containment | YES | MEDIUM | No | *(Verifier crashed in WF-1; re-grounded in WF-2 — see §6.)* Four families of process/fs sinks consume DB/analysis-derived paths with no containment validation. |
| **F6** emit-before-gate | YES | LOW | No | *(WF-1 verifier produced degenerate output; re-grounded by the spec author.)* `execution_complete_http` emits external `outcome=completed` + webhook (`steps.rs:764-792`) before the gate decides PendingReview vs Failed. |
| **F7** prompt/tool contract | Sub-claim 1 YES / **Sub-claim 2 NOT-A-BUG** | MEDIUM | No | Worker prompt's no-tests case sends `test_result:{tests_ran:false}` → Rust deser 422. Codex harness-native delegation sub-claim is **not** a defect (`delegate_start` is ideation-scoped; worker lacks the grant — leave as-is). |
| **F8** docs stale | YES | LOW | No | Handoff docs frame already-shipped fixes as TODO; doc 04 wrongly claims `transition_task_corrective→Merged` auto-unblocks dependents (corrective transitions skip `on_enter(Merged)`). |
| **F9** test-quality gaps | YES | MEDIUM | No | Setup-before-spawn covered only by helper-level tests bypassing `on_enter`; `err.to_string().contains()` instead of typed `AppError::ExecutionBlocked`; missing negative-metadata assertions on rescued tasks. |
| **F10** reconciliation stale-failed repair | YES | LOW | No | No path clears `validation_cache` on restart/auto-retry/reset; startup recovery hardcodes `(TransientTimeout, Timeout)`, dropping `AgentIncomplete→IncompleteSteps`. |

**Data-quality notes (reported honestly):** the F5b verifier exhausted its structured-output retries and returned nothing; the F6 verifier returned schema-valid but placeholder content. Both findings were re-grounded later (F6 by the spec author during drafting; F5b by a dedicated WF-2 agent). Their entries above reflect the re-grounded reality, not the failed runs.

---

## 4. Stage 1 — Design Debate

Three competing architectures were generated independently, then judged:

| Lens | Strategy | Core idea | Judge score |
|---|---|---|---|
| A · minimal-surgical | Attempt-Boundary Invalidation | Clear completion artifacts on entry + one shared latest-entry guard | — |
| B · run-identity-first | **Run-Identity-First Completion Seam** | One attempt-authority resolver + run-stamped evidence + post-gate idempotent emission | **8.7 (winner)** |
| C · fail-closed | Attempt-Stamped Completion Seam | Fail-closed run-identity spine; require positive signals everywhere | — |

The judge selected **B** as the spine and grafted 7 ideas from A and C (notably the latest-entry guard from A and the fail-closed step gate from C). The round-1 draft spec therefore proposed: a `resolve_current_execution_attempt` resolver, **run/chain stamping** of `ValidationCacheMetadata`, a tri-state step gate, `completion_tool_called` plumbing — **and an unconditional `on_enter` cache clear** as the "primary" reviewer-leak closure.

---

## 5. Stage 1 — Adversarial Convergence (and the self-correction)

Two dual-lens critic rounds attacked the draft. This is where the most important work happened.

**Round 1 (8 gaps, 3 blocking):**
- Run-stamping silently breaks currently-passing finalizer rescue tests (fixture has no `agent_run_id`) and contradicts the spec's own green-run regression guard.
- `IdentityUnknown ⇒ proceed-as-current` lets the repo-free cache rescue fire for a stale/ambiguous attempt → **this gap became finding F1b.**

**Round 2 (7 gaps, 4 blocking) — the critical catch:**
1. **CRITICAL — the unconditional `on_enter` cache clear reintroduces the exact false-negative the branch fixes.** Recovery/shutdown re-drive (`execute_entry_actions`, `startup_jobs.rs`) fires `on_enter(Executing)` on a *still-executing* task holding its **own** green cache; the clear wipes it → worker re-runs from scratch → trapped on the lingering `failed` step. This directly violates the project rule *"Trust GREEN validation_cache on HEAD + execution_complete; don't blindly re-run."*
2. **HIGH — stamping + clear is contradictory/over-engineered.** Run-stamping never reaches the HEAD-only reviewer reader, so the design leaned on the (unsafe) clear for that leak.
3. **HIGH — `get_status_last_entered_at` cited a non-existent trait path and undercounted impls** (3 claimed vs. 9 real) → would not compile.
4. **HIGH — the "necessarily a PRIOR attempt's cache" invariant is false under shutdown-resume** (the `handle_stream_success` shutdown guard leaves the task Executing with its own green cache; `persist_shutdown_interrupted_metadata` doesn't set `preserve_steps`).

### The pivot: stamping + on_enter clear → episode-freshness gate

The final revision resolved all four round-2 gaps with **strictly less surface**:

> A green, HEAD-matched cache is trusted by a completion/reviewer reader **iff** `cache.captured_at >= get_status_last_entered_at(task, latest Executing/ReExecuting entry)`.

Why it works:
- **No struct migration** — `ValidationCacheMetadata.captured_at` already exists.
- **Recovery-safe** — `execute_entry_actions` re-runs `on_enter` *without a transition*, so it appends **no** status-history row; a shutdown-resume leaves the episode boundary unchanged → a same-attempt cache stays trusted → no reintroduced false-negative.
- **Closes the cross-attempt leak on BOTH readers** — a genuine re-execution appends a new history row at `T_new`; a prior cache (`captured_at < T_new`) is rejected. A temporal check is reader-agnostic, where a run-stamp could not reach the reviewer reader.
- **One mechanism, no destructive clear.** Run/chain stamping and the on_enter clear were **deleted from the plan.**

---

## 6. Stage 2 — Independent Confirmation

The pivot was the spec's load-bearing decision but had been *proposed and self-verified by the same agent*. WF-2 supplied the missing independent review.

### Pivot skeptic — verdict: `pivot_sound_zero_blocking`
An independent agent, told to *refute* the gate against live code, **could not break it**:
- **(a) No reintroduced false-negative** — confirmed `execute_entry_actions` (`task_transition_service.rs:2925`) appends no history row; the self-transition guard makes a second same-status row structurally impossible without first leaving the status.
- **(b) No cross-attempt false-positive** — a genuine re-execution must leave-then-return via `transition_task` (new row at `T_new`), and a prior cache is always captured before that prior episode's exit, which precedes `T_new` → `captured_at < T_new` strictly.
- **(c) No compile/contradiction blocker** — `captured_at` is a required pre-existing field; the latest-entry query is writable; both readers are reachable.

It found **two LOW, non-blocking spec-accuracy bugs**, both corrected:
- **Gap A — granularity:** the spec called `created_at` second-granular; production writes `Utc::now().to_rfc3339()` (sub-second). The design is therefore **safer** than documented — the "same-second" residual collapses to an impossible exact-nanosecond tie, and the `rowid DESC` tiebreaker is defensive-only.
- **Gap B — symbol name:** the reviewer reader `is_skip_test_validation` **does not exist**; the real surface is `compute_validation_cache` (async, `helpers.rs:1606`) + `compute_validation_hint` (`:1562`), a **shared get_task_context hint path** used by worker/reviewer/merger.

### F5b grounding (re-run after the WF-1 crash) — MEDIUM, prior spec treatment **inadequate**
Confirmed four families of currently-unvalidated path sinks fed by DB/analysis-JSON paths:
- `execution.rs` `exec_cwd` (exists()-only), the **shared** `merge_validation/mod.rs:91` spawn cwd, `merge_validation/setup.rs` (`create_dir_all`/`read_link`/`remove_file`/spawn), `merge_validation/install.rs` (`read_link`/`remove_file`/spawn), and `git_cmd.rs:107` via `get_head_sha` from `validated_completion_override` (**no check at all**).

Corrections applied to the spec:
- **Reuse the existing app-owned `crate::utils::path_safety`** (`validate_absolute_non_root_path` + `checked_*`, already the `git_service/worktree.rs` pattern) and add **one** shared `ensure_path_within(root, candidate)` — **do not** invent a task-specific `ensure_worktree_path_contained`.
- Expand the single-sink treatment to the full five-sink inventory (incl. per-entry `resolve(&entry.path)` joins and parsed symlink target/parent).
- Move the CodeQL suppression to the **actual** sinks (validate at the `cmd_cwd` construction sites so the shared `mod.rs:91` sink isn't masked while still tainted).
- Make containment **unconditional** across all `merge_validation_mode`s (Off has identical exposure); reframe F5b as hardening of pre-existing sinks, **not** a branch regression.

---

## 7. Decided Architecture (per finding)

| Finding | Decision |
|---|---|
| **F1 / F1b** | One `resolve_current_execution_attempt` → `Current{task, episode_entered_at}` / `Stale` / `IdentityUnknown`, gating finalizers off the **latest** status entry (`get_status_last_entered_at`). Cache rescue consulted **only** on `Current{…}`; under `IdentityUnknown` the override is not called. |
| **F2** | **Episode-freshness gate** (`captured_at >= episode_entered_at`) on **both** readers. No run/chain stamping, no on_enter clear. |
| **F3** | Tri-state `StepCompletionState{NoSteps, AllComplete, Incomplete, Unknown}`; `Unknown`/`Incomplete` never consult `has_output` (fail **closed**). |
| **F4** | Thread `completion_tool_called` through `StreamOutcome` into **all three** `handle_stream_success` call sites; zero-step advances only on signal-or-validation. |
| **F5a/F5b** | Surface hard setup failures (mode-gated block); **unconditional** worktree/analysis-path containment via `path_safety` + new `ensure_path_within`. |
| **F6** | Compensate (emit a `Failed`-outcome external event) rather than hard-reorder; **deferred** (internal state already idempotent). |
| **F7** | Prompt-only: omit `test_result` when no tests ran. No Rust/MCP schema change, no rebuild. Sub-claim 2 untouched. |
| **F8 / F9 / F10** | Docs correction; on_enter-level + typed-error tests; cache-lifecycle clears on **no-in-flight-run** paths only + startup source fidelity. |

---

## 8. Merge-Blocking Subset & Sequencing

**Land as ONE coordinated change set (this is what makes the branch merge-ready):**
1. Helpers: `H1` `get_status_last_entered_at` (corrected trait path at `crates/ralphx-domain` + sqlite/memory + all 7 test doubles), `H2` resolver, `H4` `validation_cache_fresh_for_episode` (no struct migration), `H5` tri-state + `StreamOutcome.completion_tool_called`.
2. **WI-1** (F2 + F1b) · **WI-2** (F3) · **WI-3** (F4) — one coordinated edit of `chat_service_handlers.rs` (success block `1010-1074` + AgentExit override `2476-2496` + the two internal Cancelled-as-success calls `1605`/`1685`) **plus** the shared get_task_context reviewer reader.
3. Update existing rescue/override tests for the episode gate; add the shutdown-resume regression test; zero clippy warnings; zero test failures.

**Then, same branch, can follow (parallelizable where files are independent):** WI-4 (F1), WI-5 (F5a/F5b), WI-9 (F10), WI-6 (F7, prompt-only), WI-7 (F8, docs), WI-8 (F9). **Deferred follow-up:** WI-10 (F6).

Full atomic commit plan, exact symbols/line anchors, and tests-first names are in **`implementation-spec.md` §4 and §6**.

---

## 9. Explicitly Rejected Alternatives

| Rejected | Why |
|---|---|
| **Run/chain stamping of `ValidationCacheMetadata`** | Struct migration + ~6 test migrations; does **not** reach the HEAD-only shared reviewer reader; creates a shutdown-resume false-negative if recovery re-drive doesn't continue the run chain. Superseded by episode-freshness. |
| **Unconditional (or any) `on_enter` cache clear** | Fires during recovery re-drive on a still-Executing task holding its own green cache → **reintroduces the exact false-negative the branch removes.** |
| **Relaxing the Rust/MCP `tests_passed` schema (F7 option c)** | Would force an MCP rebuild; prompt-only fix chosen instead. |
| **New `ensure_worktree_path_contained` helper** | An app-owned `path_safety` helper already exists; reuse it + add one `ensure_path_within`. |
| **"Fix" Codex harness-native delegation (F7 sub-claim 2)** | Not a bug; `delegate_start` is ideation-scoped by design. |

---

## 10. Residual Risks (LOW, accepted/deferred)

- **Same-nanosecond cross-attempt tie** — episode-freshness uses `>=`; a prior cache captured in the exact instant a new episode is entered would pass. Sub-second precision makes this effectively impossible across two distinct attempts; errs toward the already-accepted false-positive class. Optional follow-up: monotonic per-episode sequence for strict `>`.
- **F6 external "completed" event can precede a later Failed gate** — internal state is idempotent; tracked as WI-10.
- **Zero-step worker killed in the completion grace window** before its `tool_use` block is observed → Failed. Mitigated by the in-stream marker and `validation_complete` rescue.
- **Maintenance constraint (load-bearing):** the gate's recovery-safety rests on `execute_entry_actions` appending **no** status-history row. Any future change that makes recovery re-drive perform a real `Executing→Executing` transition breaks the gate — guarded by `test_shutdown_interrupted_resume_preserves_same_attempt_cache_rescue` and must be re-checked on any reconciliation/startup-recovery change.

---

## 11. Artifact Index

| File | What it is |
|---|---|
| `report.md` | Stage 0 — original 20-agent investigation aggregate |
| `tracker.md` | Stage 0 roster/findings + appended **Final Convergence Round** |
| **`implementation-spec.md`** | The handoff: helpers (H1–H7), per-finding work items (WI-1…WI-10), sequencing/commit plan, acceptance criteria, residual risks, §10 final confirmation. **Convergence status: CONVERGED.** |
| **`debate-and-convergence-report.md`** | *(this file)* — narrative/audit of the two workflows |

---

## 12. Convergence Statement

The merge-blocking spine (the episode-freshness pivot + identity-gated rescue + tri-state fail-closed gate + zero-step signal) **survived independent adversarial refutation with zero blocking gaps**. The single residual gap entering the final round (F5b path-containment, a non-blocking WI-5 spec-completeness issue) was corrected in the spec. **No CRITICAL/HIGH/MEDIUM gap remains unaddressed. The spec is converged and ready for implementation handoff.** No production code has been modified.
