# Remote parity — what needs your decision

**State:** Waves A–E complete at `5af02c8dc` (PR #899). Working tree clean. Nothing is running.
Everything below is either a call only you can make, or an operational blocker I cannot clear.

---

## 0. Not a decision — but it blocks everything

### CI has been dead for ~34 commits

GitHub Actions has created **zero workflow runs since `b768692dd`** (2026-08-04 ~20:07 UTC).
Actions is enabled, the workflows are `active`, GitHub status is green, and pushes are
landing — runs simply are not being created. That signature points at an **org-level minutes
or spending limit**, which needs your account access.

Consequence: every commit since is verified **only on my machine** — facade suite 419/0, full
frontend suite 787 files / 12,823 tests, layering, drift, typecheck. That is real evidence but
it is not CI, and one known gap rides on it: the last real CI run failed its Rust lib shards
on a nextest **global timeout** (393 of 6,094 tests never ran), which is a runner-capacity
symptom I could not re-judge without runs.

**Ask:** unblock Actions, then let a full run land before merge.

---

## 1. Ticketing — may a paired client spend the host's credential?

**Recommend: YES.**

The thing that blocked ticketing was never the credential. It never crosses the wire: settings
hold a `token_secret_ref` (a keychain key), resolved inside the service against a `SecretStore`.
All 20 commands sat behind one **unaudited module default**, and no command carried an
individual review.

Five zero-credential reads already shipped in Wave D1. What remains splits cleanly:

| | Commands | What it costs you |
|---|---|---|
| **D3b — six reads** | containers, tickets, filter options, ticket detail, transitions, labels | A paired device consumes the host's provider **rate limit** |
| **D3c — eight writes** | transition, assign, clear-assignee, comment, labels, two catalog syncs, presentation | Comments and transitions land in the ticket system **attributed to the host's user** |

**Blast radius today is ~0**: `ticketingDashboard` is a feature flag defaulting **false**.

**Cost if yes:** ~M each; mechanical once the boundary is set.

---

## 2. `set_update_channel` — may a paired client move the host's release train?

**Recommend: YES, with eyes open.**

The body is two lines (one repository write) and the ledger itself records that the refusal is
about authority, not hygiene. But the consequence is concrete: switch the host to `nightly`,
the host auto-updates, **restarts, and kills every running agent**.

Related and already fixed separately: the host's channel no longer drives the *client's*
updater (that was Tier 0 #2).

**Cost:** S. One ledger row (the `HostManagement` capability must be dropped — `class_permits`
rejects it under `AgentControl`), one registry row, retire a one-member test rung, regenerate,
and swap a host-only notice for a gate in the settings pane.

---

## 3. Automation brakes → `operate`

**Recommend: DEFER.**

Mechanically sound — pause is one CAS, and stop's `Stopped → Active` rollback restores the
exact pre-call value rather than re-arming (the ledger records this finding verbatim). But:

- It is the **most expensive** slice left: nine touch points, and the scope-suite brakes loop
  needs a **live automation fixture** it does not have.
- The gain is confined to **agent-revoked pairings**, because `ui:agent` is granted by default
  at pairing.
- It forces a second question you would have to answer anyway: once they are `operate` they
  **qualify** for the deliberately-closed `INERT_AFFORDANCES` list. Adding them is a boundary
  change; not adding them leaves the client stricter than the host.

**If you want it anyway, say so and I'll take it with the inert-list question answered
explicitly rather than inferred.**

---

## 4. Repository remote URLs — low stakes

Wave C2 carries capability to clients as `kind` + `has_remote`, deliberately **without** the
fetch/push URLs, because that module's contract promises no repository paths or remote URLs.
Consequence: the Remote URL row is blank on a paired client.

**Recommend: your call.** One-field widening if you want it populated; I took the conservative
read because the module said so, but your standing rule ("the client sees what the host has")
arguably points the other way.

---

## 5. Three slices that each carry their own call

| Slice | The decision inside it | Size |
|---|---|---|
| **Attachment bytes** | May attachment **bytes** traverse `remote_fetch`? Today its envelope carries a `String` body, so binary needs a new variant or a dedicated command. Metadata already ships; this is content. Path containment is solvable (rebuild from the DB row via the hashing builder, never the client-supplied path) | M/L |
| **Publish intent twin** | Publish is not fire-and-forget — it is base selection, PR-description precompute, autofix/auto-merge defaults, conflict and repair recovery. An intent row captures the *decision* but not the *dialogue*. **What should a client show between request and terminal result?** | L |
| **Close-PR intent twin** | It closes a remote PR **and terminalizes the workspace runtime**. Destructive, and adjacent to the delete-floor question | L |

---

## 6. One recommendation to change nothing

**Leave the `delete_` floor standing.**

I had repeated the assessment's "~100 rows" — it is **9**. Of those: one was mis-caught and is
now renamed (`archive_task_proposal` — it always archived), two have **no frontend callers at
all**, one was already solved by the queue twin, two are independently floored for other
reasons (`delete_chat_attachment` writes arbitrary paths; `delete_automation` does a real
`git push` to delete a remote branch), and three are genuinely destructive. Nothing dismissive
— no notification or read-marker — is caught by the prefix.

**Unless you object, this stays as-is.**

---

## 7. Queued engineering — no decision needed, just scheduling

These are ranked by how misleading they are today, and none is blocked on you:

1. **Unknown-outcome reconciliation** — the seam exists but has **2 consumers out of 178**
   `useMutation` sites. The other 176 treat `REMOTE_TIMEOUT_UNKNOWN` as an ordinary failure and
   invite a retry that the host's dedup will either double-apply or race. The fix is one
   `MutationCache({ onError })` on the already-per-environment query client. **M.**
2. **Read-only mode reaches almost nothing** — `useEnvironmentWritable` has exactly one
   consumer, and **78 of 123 registered write ops have no affordance row**, so during
   reconnect/offline those controls stay live and throw. **M.**
3. **Settings sweep pass 2** — six sections with zero remote awareness over an all-denied
   command set (Granola is the only integration panel with no host-only notice); Save/Delete
   controls that throw in Setup & Validation and Models. **M.**
4. **Global execution write twin** — the pane currently keeps rendering an edit the host
   rejected. Wave E4 made it honest; the twin would make it work. **M.**
5. **A pre-existing test failure**, proven not ours:
   `git_auth_tests::repository_capability_uses_git_effective_urls_for_included_push_rewrites`
   passes alone, fails in its batch, and fails identically at `f20a675a3` in a clean worktree.
   Unfixed; a root cause would be welcome.

---

## Fastest path to merge

1. **Unblock Actions**, let a full run land, and judge the nextest global-timeout shard.
2. Answer **§1 and §2** (both recommended yes, both mechanical once decided).
3. Say **defer or do** on §3, and **yes or no** on §4.
4. §5 can wait for a separate wave; §6 needs only your non-objection; §7 I can start any time.

An adversarial review brief for the whole of A–E is at
`docs/handoffs/remote-parity-waves-a-e-review-prompt.md` — worth running before merge, given
that none of this has been through CI.
