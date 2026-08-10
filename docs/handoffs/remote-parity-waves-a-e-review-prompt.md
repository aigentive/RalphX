# Adversarial review request — remote parity Waves A–E

You are reviewing ~26,500 added lines across 332 files on `feat/remote-multi-env` (PR #899),
worktree `~/Github/ralphx-worktrees/remote-multi-env`. **Base `7f90edc38` → head `5af02c8dc`.**

**None of this has ever run in CI.** GitHub Actions has created zero runs since `b768692dd`
(suspected org minutes/spending limit), so every claim below rests on local verification by
the author of the code. Treat "the author says it passes" as an unverified claim, not
evidence. That is the single most important fact about this review.

---

## The standing requirement everything was built against

> *A paired remote client must be able to see and manage everything the host can, and the host
> running alone — with no client connected — must behave exactly as before.*

The second half is as binding as the first. **Local behavioral parity is the invariant**:
local command bodies, TTLs, and return values were supposed to stay byte-identical, with
remote support added only additively.

---

## Architecture you need to know to review this

- **The facade is an allowlist.** `src-tauri/src/remote_server/registry.rs` is exhaustive over
  `generate_handler!`; every `:3849`-reachable command has a hand-audited row plus a class in
  `capability_ledger.rs` (Read / Operate / AgentControl / Elevated / Denied). A command absent
  from the generated manifest is `unavailable` to clients — **absence is the signal**, never a
  hardcoded name list.
- **Intent-twin pattern** (used ~15 times): a spawn-reaching local command cannot be
  registered, so a `request_remote_*` command persists an intent row fail-closed; a host-owned
  dispatcher in `application/startup_background.rs` claims it (CAS Pending→Starting),
  re-proves authority, executes the local seam, and persists a distinct terminal code. Stale
  claims sweep to `FailedStale` and are **never retried**.
- **Cached-shell-out pattern** (Wave C): a read that incidentally shells out is served from a
  host-written snapshot by a twin that never populates it.
- **Layering ratchet** (`python3 scripts/check-layering.py`): `application/` may not import
  `commands/` or `http_server/`.
- **Detectors**: the ledger tests measure each command's call-graph closure for process
  launches, arming writes, and content mutation. They are call-graph based, not runtime based.

---

## What each wave claimed to deliver

| Wave | Commits | Claim |
|---|---|---|
| **A** | (pre-`7f90edc38`, plus `b768692dd`) | Tier 0 live defects: a client could halt the host's scheduler with no way to restart; the host's release channel drove the *client's* auto-update; permission notifications went nowhere; an unanswerable modal was pushed to clients |
| **B1** | `8fc1d5bfe` | Execution/task/group resume + restart intent twins with dispatchers |
| **B2** | `ab33edbc4` `d4d028ddf` `6ee25f771` `47093d004` | **The headline gap**: plan approval, ideation accept/reject finalize, plan-document edit — plus the frontend that makes them usable |
| **B3** | `2dfd14514` `4bd69504b` `6ec7cc241` | Queue management: cold-hydration read, cancel, send-now (a kill + launch) |
| **B4** | `6f0c8e408` `cb3f6d829` `15874b7af` `5d8bd8075` `274f1496e` | Automation: setup-edit, run-now, retry-judge, create-draft |
| **B5** | `edbaa6e16` `abcc1af61` `fe0ee529f` | Conversation lifecycle: mute, persona, unarchive, recovery-prompt resolution (Tier 0 #5), archive, fork |
| **C** | `f20a675a3` `585fd5920` `fc6ad3b3e` `2b71f1339` | Cached reads: honesty pass, repository capability, MCP catalog, workspace changes, file diffs (diff domain went 0/29 → working) |
| **D** | `b8d22a89b` | Reclassifications: host version surfacing, five audited ticketing reads, a spawner misclassification, `delete_task_proposal`→`archive_task_proposal` |
| **E** | `52187304d` `a99df33a1` `1909fd2a4` `5af02c8dc` | Gate-row regression + guard extension, the `catch(()=>[])` sweep, attachment metadata twin, publish surface |

---

## Where to attack — ranked by where this work is most likely wrong

### 1. Idempotency and at-most-once claims (highest value)
Several twins claim at-most-once by a **pre-allocated id** minted at request time:
`request_remote_automation_draft` (AutomationId) and `request_remote_conversation_fork`
(child ChatConversationId). The claim is that a re-entrant claim finds the entity present and
settles benign **without calling the seam**.

- Is the pre-allocated id actually used by the seam, or does the seam mint its own anyway?
- Is the "already exists" check racing the create (TOCTOU), and does it matter given the PK?
- `request_remote_queued_message_send` is a **kill + launch** — it stops a live provider and
  starts a fresh turn. Prove a replay or a stale claim cannot kill a second agent.
- Every dispatcher claims stale claims are never retried. Verify that in code, not comments —
  a half-run archive has already deleted a worktree and branch.

### 2. Authority re-proof at claim time
Each dispatcher must re-prove authority *after* claiming, because the world moves between
persist and drain. Check specifically:
- `request_remote_conversation_archive` re-proves `close_pull_request` against the **live**
  workspace (a workspace can become a Review-PR in the window and must never close the PR it
  is reviewing).
- `request_remote_plan_approval` forces `PlanApprovalActor::User` **by field absence** — the
  wire has no actor field. Confirm no path lets a client influence it.
- The recovery-prompt twin re-proves both the handled-status set and the live prompt marker.
- Where a repo read fails at claim time, does it **propagate** (leaving the claim for the
  sweep) or collapse into a terminal? Collapsing a transient error into a terminal refusal is
  the bug pattern to hunt.

### 3. Host-alone behavioral parity (the invariant most likely quietly broken)
Waves B–E extracted many `*_for_state` seams and re-homed several across layers, and Wave C
added write-through caching to paths the host already executes.
- Did any extraction change local behavior, ordering, or error semantics?
- Wave C4a/C4b added a **separate 24h remote snapshot TTL** and swore the local 2s review TTL
  and 30s PR-annotations TTL were untouched. Verify.
- Wave C2 writes a capability cache through `project_response`. Confirm the local return value
  is unchanged and the local path never *reads* from the cache.

### 4. The detectors' blind spots
A load-bearing finding from Wave B5b: expressing a spawn-distinct difference as **one function
behind a boolean** puts the spawned side in the twin's call-graph closure regardless of
runtime, because detectors are static. The fix was two distinct functions
(`.claude/rules/remote-facade.md`).
- Look for the inverse: a place where an indirection (closure, bare fn value, trait object)
  **hides** a real launch from the detector, making an unsafe row look clean. The ledger
  already documents two such hand-traced misses around `resolve_claude_cleanup_cli` and
  `remove_reserved_user_registration`.
- Wave D found `start_ralphx_work_from_ticket` — a process spawner masked as
  credential-deferred by a module default, invisible to the detector in that position. **Are
  there others?** Module defaults were never audited per command for most modules.

### 5. Absence rendered as fact
The recurring defect class this effort kept finding (~20 instances, two of which lied on the
**host** as well as remotely). Fixed in Waves C1/C5; the vocabulary is an explicit
unknown/unavailable state distinct from empty.
- Find remaining instances. Particularly: a failed read becoming `[]`/`null` and then being
  **acted upon** rather than displayed — Wave E5 found one where a dropped `baseBranchOverride`
  re-applied proposals onto the **wrong base branch**.
- Check the new snapshot envelopes: does `snapshot: null` reliably reach the UI as
  "not captured" rather than "empty"?

### 6. Gate/op correspondence
Wave E1 fixed a regression where registered twins were unreachable because gate rows still
named retired local commands — **it re-closed a Tier 0 defect at the UI layer**. The
`agent-gate-op-consistency` guard now covers 54 files.
- Are there gate rows still naming ops that no longer exist, or gating op X while the handler
  invokes op Y? (Two such were found; assume more.)
- Are there registered write ops with **no** affordance row at all? The scoping counted 78 of
  123 — those controls stay live during reconnect/offline and throw.

### 7. Test quality
Delegated agents repeatedly wrote tests asserting shapes the code cannot produce — an invented
`{outcome:"error"}` envelope (the real one is `{outcome:"commandError", error:<unwrapped>}`),
whole-tuple invoke assertions that pin arity, and payloads in the wrong casing. Several were
caught; assume some survived.
- Do the absence assertions actually assert absence, or merely pass because nothing ran?
- Do dispatcher tests exercise production entry paths, or a hand-rolled substitute?

---

## Known-and-accepted (do not re-report as new)

- `git_auth_tests::repository_capability_uses_git_effective_urls_for_included_push_rewrites`
  fails inside its batch, passes alone. **Proven pre-existing** at `f20a675a3` in a clean
  worktree. Still unfixed — a root cause would be welcome, but it is not a regression.
- Rust tests need `RUST_MIN_STACK=8388608` (CI parity) or an unrelated publish-recovery test
  aborts the binary.
- Owner decisions deliberately parked (do not treat as omissions): ticketing credential spend,
  `set_update_channel` reclassification, automation brakes → `operate` plus the closed
  `INERT_AFFORDANCES` question, C2 remote-URL widening, attachment bytes over `remote_fetch`,
  publish/close-PR intent twins.
- The `delete_` prefix floor stays: of its 9 rows, one was mis-caught (renamed), two are dead,
  one was solved by a twin, two are independently floored, three are genuinely destructive.

---

## How to verify (do not trust the commit messages)

```bash
cd ~/Github/ralphx-worktrees/remote-multi-env
python3 scripts/check-layering.py
cd src-tauri && RUST_MIN_STACK=8388608 cargo check --tests --features test-utils   # expect zero warnings
RUST_MIN_STACK=8388608 cargo test --lib --features test-utils remote_server::      # expect 419/0
cd ../frontend && bunx vitest run                                                  # expect 787 files / 12823 tests
set -o pipefail && bun run typecheck
cd .. && node scripts/check-remote-transport-drift.mjs .                           # 616 names, 0 unclassified
cargo run --manifest-path scripts/event-manifest-scanner/Cargo.toml -- --check
```

Per-slice implementation notes are in `src-tauri/.codex-p*-tracker.md` (one per slice — they
record what was attempted, what was blocked, and the decisions taken). The roadmap and its
owner-decision table are in `docs/handoffs/remote-parity-assessment.md`.

---

## What I want back

Concrete counterexamples with `file:line`, ranked by severity, in these buckets:

1. **Correctness** — a client can cause the host to do something wrong, twice, or to the wrong
   target. Include the sequence that triggers it.
2. **Host-alone regression** — anything where local behavior changed.
3. **Dishonest UI** — absence or failure still presented as fact.
4. **Classification** — a row whose class or capabilities the code does not support, or a
   spawn/authority the detectors cannot see.
5. **Test theatre** — tests that would pass if the behavior regressed.

Do **not** confirm the design first, and do not summarize what the waves did — I wrote them
and the commit messages already say it. Assume the author was systematically over-confident
and look for what that would have hidden. If a whole wave is sound, say so in one line and
spend the effort elsewhere.
