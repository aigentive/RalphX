# Remote Coverage — Implementation Handoff

**Source:** `docs/reviews/remote-coverage-adversarial-review.md` (2026-08-01, branch `feat/remote-multi-env`)
**Status:** ready to implement · phases ordered top priority → lowest · each phase is independently shippable

This handoff turns the review's 17 confirmed findings + 10 cross-cutting findings into an ordered build plan. It deviates from the report's own priority list in four places, each explained inline under **Direction change**. The one-sentence thesis: *the remote feature's read plane is done; the write plane fails not because gates are missing as a concept but because the existing gate machinery is pointed at the wrong ops, never consumed, or bypassed by whole surfaces — so the cheapest highest-leverage work is making the machinery **verifiable**, then sweeping surfaces onto it.*

## Phase map

| Phase | Theme | Size | Ships user-visible value |
|---|---|---|---|
| 0 | Guardrails: make gate wiring falsifiable | ~½ day | No (but multiplies every later phase) |
| 1 | Restore the safety contract (brakes + questions + queue) | 1–2 days | Yes — permission/question gates work remotely |
| 2 | Close the Tauri-plugin side door | ~1 day | Yes — links open on the right machine, notifications toggle works |
| 3 | Unblind liveness, kill the poll storms | 2–3 days | Yes — running state + Stop button + halt banner on the client |
| 4 | Honest gating sweep (automations, task details, plan, chat tail) | 3–5 days | Yes — enabled-but-doomed buttons become honest disabled states |
| 5 | Truthful attribution (ticketing / GitHub copy about the host) | 2–3 days | Yes — the UI stops lying about the host's configuration |
| 6 | Product decisions + protocol extensions (design-first) | 1–2 wks | Yes — plan approval remotely, unknown-outcome safety |
| 7 | Sweep the unswept domains | ongoing | Audit output feeding phases 4–6 |

---

## Phase 0 — Guardrails first

**Direction change #1 (vs. report priority list):** the report puts fixes first and tests inside each fix. Do the guards *before* any fix. Two confirmed criticals ("Run now" gated by the wrong op; PlanEditor's gate resolving an op its save path never calls) lived in files the wiring guard **certified as correctly gated** — the guard only asserts a file imports `useAgentGate`, never that the resolved op matches the invoked command. Every gate added in phases 1–5 lands under the same blind guard unless we fix it now.

Work items:

1. **Op↔callsite consistency test.** New static test alongside `frontend/src/components/remote/agent-gate-surfaces.test.tsx`: for every `AGENT_GATED_AFFORDANCES` row, (a) at least one production file calls `useAgentGate("<row>")` — kills dead rows (`automationRunNow`, `automationRestart`, `folderReferenceRemove` are dead today); (b) in each file that resolves an affordance, the command names it invokes include the row's op (or a declared alias) — kills wrong-op gating. AST-lite (regex over source, same style as the existing guard) is acceptable; perfect resolution is not required, an allowlist for indirection is.
2. **Never-invoke-the-raw-twins test.** The facade splits `resolve_permission_request` → `approve_/deny_permission_request` and denies `resolve_user_question` in favor of `answer_user_question`. Add a test asserting the raw names never appear in a production `invoke(` in `frontend/src` (they may appear in `LOCAL_ONLY_COMMANDS`-style declarations). This turns Phase 1's fix into a ratchet.
3. **Extend the wiring-guard file list** to `frontend/src/components/agents/task-details/detail-views/*` (the ungated fork the Agents pane actually renders — critic critical #2). The guard will go red; that red is Phase 4's worklist, so mark the new entries as `todo`-style expected failures or land the list extension in the same PR as Phase 4's first slice — your call, but the list must not silently omit the fork again.

Exit: new tests exist; the two wrong-op findings are reproduced by a failing test before any fix lands.

## Phase 1 — Restore the safety contract

The core promise — "viewer with brakes" — is broken at every brake. All fixes are frontend rerouting onto ops the host **already registers**; no host changes.

1. **Permission gates** (`frontend/src/api/permission.ts:46`): route approve → `approve_permission_request`, deny → `deny_permission_request` under a remote environment (local keeps `resolve_permission_request`). Deny is `operate`-class — it must work on a *default* pairing; approve is `agentControl`. The gate rows (`permissionApprove`) already point at the pinned ops — after this fix they'll finally describe reality.
2. **Question answers** (`frontend/src/hooks/useAskUserQuestion.ts:279`, `frontend/src/api/ask-user-question.ts:67`): the `requestId` branch routes to `resolve_user_question` (Elevated/SpawnsProcess — unreachable at every scope). Route to the registered `answer_user_question` remotely. Then fix the failure handling: a transport refusal must **not** render "Agent session expired" and must **not** clear the question banner over a still-blocked agent — that's a false-terminal write from a non-authoritative error (stateful-workflow rule: fail closed on reads).
3. **Queued-message delete/edit fail closed** (`frontend/src/hooks/useChatActions.ts:485-496, :557-596`): both swallow the host failure after (or while) mutating local state; edit then re-sends unconditionally → the agent receives both turns. Under remote: attempt the host op first, keep local state on failure, surface the error (the `handleSendQueuedMessageNow` path already does this correctly — mirror it). `delete_queued_agent_message` is ledger-denied, so remotely these become *gated* affordances (add rows) until/unless a spawn-free queue twin is registered (Phase 6 candidate).
4. Tests: production-entry-path tests per `stateful-workflow-review.md` — assert the pinned ops are invoked remotely, assert absence of the bad effects (banner cleared, local queue mutated on failure).

Exit: on a paired client — deny works with default scopes, approve/answer work with `ui:agent`, a failed queue edit leaves exactly one truthful queued chip. Phase 0's ratchet tests keep it that way.

## Phase 2 — Close the Tauri-plugin side door

**Direction change #2:** the report ranks this #3; it goes ahead of the gating sweep because it is *actively wrong today* (not merely dishonest): "Open in browser" opens on the **host** Mac, `plugin:updater|check` asks the host about updates, global shortcuts try to bind on the host, notifications permission probe rejects and silently short-circuits a registered settings write. Small, contained, high blast radius.

1. **Routing policy** (`frontend/src/lib/remote/local-only-commands.ts:31`): add a `plugin:` rule. Default `plugin:*` → **local** (the plugins operate on *this* device: opener, dialog, fs pickers, updater, process, global-shortcut, notification), with an explicit reviewed exception list if any plugin call must target the host (none identified by the review). One prefix rule beats 77 per-import fixes.
2. **Census visibility** (P-11 drift scan + `docs/generated/remote-coverage-census.md`): teach the scan that `plugin:` names exist and are classified by the prefix rule, so the census's "0 unclassified" claim becomes true again instead of blind.
3. **Notifications toggle** (`frontend/src/components/settings/NotificationSettingsPanel.tsx:150-192`): with the routing fixed the permission probe runs locally; also add the missing `.catch` on the mount-time probe and stop `void`-discarding the toggle promise so failures surface.
4. Sanity pass over the 29 `openUrl` sites: after the fix they open on the client, which is correct for URLs (PR links, docs, OAuth). Anything that opens a host *filesystem path* must instead use the existing `HostPathCopyButton` degradation pattern.

Exit: with a remote environment active, links open locally, the updater checks the client, the notifications toggle persists; a census test enumerates `plugin:` routing.

## Phase 3 — Unblind liveness, kill the poll storms

**Direction change #3:** the report offers "wire the index *or* register a status twin." Do **not** start with new host registrations. The verifier's own evidence names the in-repo seam that already solves this exact problem — `pending-gate-reconcile.ts`, built for "backend-memory state the event log cannot replay." Mirror it with the **already-registered** `get_agent_conversation_runtime_index` (`registry.rs:1791`, carries `lifecycle: running|waiting|queued` per conversation). Zero new host surface, rule-27 audit avoided entirely.

1. **Runtime-index reconcile on connect**: on every `goLive`/reconnect (same hook point as `requestPendingGateReconcile`), fetch the runtime index and write run-liveness into the chat/sidebar stores. This fixes the cold-hydrate blindness (`subscribe{afterSeq: H}` never replays pre-connect `agent:run_started`) that hides typing indicators AND the Stop button (`shouldShowStop` requires `generating`, `AgentComposerSurface.tsx:470`) — restoring the third brake without touching the host.
2. **Transport-aware polling**: `useAgentConversationRuntimeStatus` (5s, polls on error *by design* — wrong for a capability boundary), `useAgentSidebarRunningStates` (5s + swallowed errors), `useChatRecovery` (1.5s `is_agent_running`) — under a remote environment, replace their command with the runtime index or suspend them; `isRemotelyAvailable()` (`agent-gate.ts:294`) exists for exactly this and is consumed nowhere. This ends the permanent `REMOTE_COMMAND_UNAVAILABLE` poll storm eating the per-device pacing budget (8 slots / 10 rps).
3. **Execution status** (critic #6): `get_execution_status` is denied (resolves process-inspection CLI) so every consumer defaults to "running, nothing queued, may start" and the halt banner can never render — a remote user's prompts queue invisibly. The write side already has a spawn-free twin (`update_remote_execution_settings`); register the matching **read** twin (`get_remote_execution_status` from DB state, no process inspection) — this one *does* need a rule-27 hand-audited registry entry + ledger class + denial-test update, and is worth it. Fail closed in the meantime: consumers must treat an unavailable read as "unknown", never as `canStartTask: true`.
4. Tests: cold-hydrate scenario (run live on host → client connects → status shows running, Stop renders); poll-storm regression (no repeating invokes of unregistered commands under remote).

Exit: a client that connects mid-run sees the run, can stop it, and issues zero doomed polls; a stopped host scheduler shows the halt banner remotely.

## Phase 4 — Honest gating sweep

Mechanical once Phases 0–3 exist; the pattern is always the same: add/point the affordance row, consume it via `useAgentGate` (which folds in read-only mode — this is deliberately the *only* way surfaces get degraded-connection behavior, per critic #8), render the existing `AGENT_CONTROL_DISABLED_HINT` / `REMOTE_UNAVAILABLE_HINT` copy, and let Phase 0's guard verify op↔callsite. Slices, in order:

1. **Automations** (2 confirmed criticals): fix the "Run now" wrong-op gate (`AgentsAutomationPanel.tsx:654/1113/1424` → consume `automationRunNow`); gate the ungated judge-retry beside it; sweep the Automations page's 12 call sites (`AutomationDetailView/Header/RunsTab/RunTimelineItem`) with rows for pause/stop/cancel-run/resume-run/skip-judge/plan-judge-retry/settings-edit and unavailable rows for run-now/judge-retry/delete-automation/delete-run/setup-edit. Fix the two brake-comment lies: pause/stop are host-classified `AgentControl`, so either re-class them host-side to `operate` (they reduce authority — defensible, needs the rule-27 audit) **or** gate them as agentControl and delete the "brakes boundary" comments; pick one, don't leave the contradiction. Fix the notification resume action's false "no longer resumable" copy (`notificationNavigation.ts:62`).
2. **Agents task-detail fork** (critic critical #2): port the twins' `useAgentGate` wiring into all 13 `components/agents/task-details/detail-views/*` files (`taskApprove`, `taskUnblock`, merge rows), add rows + gates for `retry_merge`/`resolve_merge_conflict` in **both** copies, and skip the optimistic `pending_merge` cache write when the gate isn't enabled. Then flip Phase 0's guard entries from expected-fail to enforced. (Do *not* attempt to unfork the views in this phase — the fork is a deliberate pattern per `task-detail-views.md`; unforking is a separate refactor decision.)
3. **Plan approval / ideation acceptance** (critic critical #3): gate `handleApprovePlanFromQuestion` (`AgentsActiveConversationPanel.tsx:1890-1926`), PlanEditor save (point its gate at what it actually calls — or better, port the save to `update_artifact` which *is* registered and is what its current gate claims), and `accept-finalize`/`reject-finalize` with honest unavailable copy. This phase makes them honest; **making them work remotely is Phase 6** and is flagged there as the highest-value product decision in this document.
4. **Conversation tail**: hide the mode picker on remote active conversations (the start composer already does exactly this — `AgentsStartComposer.tsx:659-667`; it's a straight inconsistency); rows for fork/archive/mute/persona-switch; gate attachments (`enableAttachments`) until a remote upload path exists — note the current path serializes whole files onto a JSON invoke the host then refuses; consume `folderReferenceRemove` at the chip's × (`AgentComposerSurface.tsx:2069-2081`).
5. **"New automation" silent rewrite** (`AgentsStartComposer.tsx:663-667` + `App.tsx:871`): a remote user clicking New automation lands in a plain chat composer with no explanation — show the unavailable hint instead of silently degrading.

Exit: zero enabled controls on a paired client whose click can only produce `REMOTE_COMMAND_UNAVAILABLE`/`REMOTE_FORBIDDEN`; Phase 0 guards enforce it structurally.

## Phase 5 — Truthful attribution

The misattribution family: reads about the *host* fail and the UI converts absence into confident wrong claims about whichever machine the user is thinking of.

1. **GitHub settings** (`GitHubIntegrationSettingsPanel.tsx:24-58`, `IntegrationsHubSection.tsx:137-152`): branch on `useIsRemoteEnvironment` (the pattern already exists at `HarnessProvidersSection.tsx:668`) and render "checked on the host — not available remotely" instead of "gh missing / Install the GitHub CLI".
2. **GitAuthRepairPanel** (`GitAuthRepairPanel.tsx:139-166`) + `useGitAuthStartupNotification`: suppress the transport-error → "git problem" inversion under remote; also fix the always-refetch query economics (`useGithubSettings.ts:60-70` staleTime 0 + focus refetch on a command that can never succeed remotely — same anti-pattern Phase 3 kills for liveness).
3. **PR Mode toggle** (`RepositorySettingsSection.tsx:277-335`): the remote project twin deliberately drops `repository_capability`, and the client renders that as OFF+disabled+"could not inspect". Either (a) have the twin carry a coarse, path-free capability kind (host-side change, small; recommended), or (b) render a distinct "managed on the host" state. Never render a durable host setting as its opposite.
4. **Ticketing**: providers pane says "Not configured" about a host that is configured, linked-ticket chips silently vanish, and the dashboard nav strands a switched user. Same recipe: remote-aware copy ("configured on the host — ticketing runs host-side"), an explicit absent-remotely chip state, and nav gating (`LeftNavRail.tsx:123-132` already gates ticketing by provider presence — make the provider read's unavailability collapse the entry rather than error the view). GitHub dashboard nav (`item.view === "github"` unconditional) gets the same treatment.
5. **PR review deep-link** (confirmed critical): the ActionRequired notification lands a remote user in a conversation with no Review tab and no explanation. Since the sidebar twin already ships per-row workspace metadata, render a lightweight read-only "PR #N — review runs on the host" notice on the conversation (hide-with-explanation), rather than nothing. PR-detail body: use `remoteErrorBannerProps` (`agent-gate.ts:340-352`, built for this, unused here).

Exit: no surface makes a false claim about the host's configuration; every capability boundary in these surfaces names itself as one.

## Phase 6 — Product decisions + protocol extensions (design-first)

Things that need a decision or a host-side surface, not just wiring. Each wants its own short design note + rule-27/28 audit before code.

1. **Make plan approval work remotely** — *the* product gap. Approving a plan from the couch is the reason remote exists; today the mandatory confirmation gate dies on an unmounted POST route. Recommended shape: spawn-free **invoke** ops (`approve_remote_plan_artifact`, ideation accept/reject twins) with `agentControl` class + dedup, per the established twin pattern — not remount-POST expansion (fetch stays read-only by construction, keep it that way).
2. **Unknown-outcome reconciliation as a seam, not call sites** (critic #6-adjacent, confirmed): `requestId` is minted per *call*, so a post-timeout re-click is a genuine second mutation; `reconcileUnknownOutcome` has 2 consumers out of ~90 mutating ops. Design: a per-facade-op reconcile registry consulted by `networkInvoke` on `REMOTE_TIMEOUT_UNKNOWN`/`REMOTE_REQUEST_IN_PROGRESS` (refetch the affected entity, block the re-click until reconciled), plus intent-level request ids for the worst offenders (`inject_task`, `create_task`, review transitions). This is transport-layer work; do it once, every op inherits it.
3. **Queue management twins** (unblocks Phase 1's gated delete/edit): spawn-free `get/delete/update_queued_agent_message` reads/writes from DB state.
4. **Execution control** — the board can pause but never resume remotely (whole-scheduler asymmetry, unswept domain #1). Decide the remote execution-control story deliberately: which of resume/restart/recover get spawn-free intent twins (the conversation-start dispatcher pattern generalizes) vs. stay host-only with honest gates.
5. **Global execution settings twin** (`update_global_execution_settings`) — the per-project sibling already has one; also fix the optimistic-keep-on-reject + unmount-flush toast (`GlobalExecutionSection.tsx:57-94`).
6. **Publish/Commit&PR**: keep host-only in v1 (verifier downgraded this to hide-with-explanation), but render the explanation; remote publish is a separate future design.

## Phase 7 — Sweep the unswept

The report's "Not swept" list is a queue of future reviews, highest value first: **diff/workspace-changes viewer** (29/29 commands denied — likely the automations story again), **Settings tree half-saves**, **projects twins field-drop audit** (what else besides `repository_capability` vanished?), **retention/cursor-resume behavior when a client's lease expired**, **host-path leakage in task/merge/artifact/automation DTOs**, **mid-session scope widening** (requires reconnect today; decide if that's acceptable and document it), **env-switch query routing race** (`invoke` resolves the transport env at call time while caches are env-keyed — audit refetch-after-switch). Re-run the adversarial workflow per domain as each is picked up.

---

## Standing rules for every phase

- **Extend the owning seams** (repo rule 0): gates go through `agent-gate.ts` rows + `useAgentGate`; new host surface goes through hand-audited `registry.rs` + `capability_ledger.rs` entries (rule 27); events through the classification table (rule 28); reads that mirror host memory go through the reconcile-on-connect pattern (`pending-gate-reconcile.ts`). No parallel gating systems, no `generate_handler!` shortcuts.
- **Fail closed** (stateful-workflow-review): an unavailable read is "unknown", never a default that authorizes progress (`canStartTask ?? true` is the anti-pattern, twice found).
- **TDD, focused validation**: every fix lands with a production-entry-path test asserting the *absence* of the bad effect; Rust changes trigger the post-test `cargo clean` rule.
- **Copy discipline**: capability boundaries use the two existing hint constants; never invent new "not available" phrasings per surface.
