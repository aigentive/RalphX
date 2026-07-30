# Remote Workspace Mirror — v1.5 handoff

**Status:** investigation complete, ready for implementation lanes.
**Symptom:** a paired client connects cleanly (green badge, stream live) and then renders the
first-run Welcome/"Set Up Provider" screen — no projects, no conversations, no automations —
even though the host has all of them.
**Date/base:** 2026-07-30, `feat/remote-multi-env` after the hydration-barrier + pacing fix
(`e80509a60`).

---

## 1. What is actually wrong (verified, not speculated)

The connection layer is healthy. The gap is exactly one tier: **the app shell's root queries
are not on the remote facade**, so the client can never render the workspace frame that all
the already-remoted surfaces hang off.

Ledger truth for what the boot path calls (`docs/generated/remote-commands.json`):

| Shell query | Ledger class | Registered | Why it's classed that way |
|---|---|---|---|
| `list_projects` | `elevated` | **no** | module-blanket "project git/gh and deferred shell authority" |
| `get_agent_provider_settings` | `denied` | **no** | "configures future provider process authority" |
| `get_execution_status` | `elevated` | **no** | module-blanket |
| `get_ui_feature_flags` | `read` | yes | — |

Meanwhile the mid-tier is **already registered and working** over `:3849`:

- Tasks/Kanban: `list_tasks`, `get_task`, `get_task_steps`, `get_active_plan`, `create_task`,
  `update_task` (reads `Read`, mutations `Operate`).
- Conversations/transcripts: the projected **remote twins** — `list_remote_agent_conversations`
  (+ `_page`), `get_remote_agent_conversation`, `_messages_page`, `_timeline_page` (`Read`) and
  `send_remote_chat_message` (`AgentControl`).
- Ideation reads (`list_ideation_sessions`, `get_ideation_session`), automations reads
  (`list_automations`, `get_automation`), `list_personas`, `list_notifications`.
- Fetch remount: session plan, plan verification, agent task lists, workflow runs.
- Events: `task:*`, `execution:status_changed`, `project:analysis_*` are classified **Durable**
  and ride the sequencer today.

So the mirror is ~80% built. The client just can't get past the front door:

1. `useProjects()` → `list_projects` → `REMOTE_COMMAND_UNAVAILABLE` → `fetchedProjects`
   undefined → `hasNoProjects === true` (`App.tsx:346`) → `WelcomeScreen`.
2. Provider settings query → unavailable → onboarding derivation runs on placeholder/failed
   data (`App.tsx:347-350`) → "Set Up Provider" copy for a host whose provider is fine.
3. Every surface behind the shell is unreachable regardless of its own registration state.

A secondary confusion: the UI reads `REMOTE_COMMAND_UNAVAILABLE` as "empty workspace". The
error is a **capability boundary** (same lesson as the hydration-barrier fix in `e80509a60`)
and must render as "not available remotely", never as first-run emptiness.

---

## 2. Owner decisions embedded in the current classification (do not casually undo)

- `create_project` / `update_project` are `Elevated` (git-init / gh at caller paths). The
  spec-amendment D3 notes they should be **pinned** Elevated-or-stronger. Remote *project
  creation* is out of scope for this handoff; "manage" means managing work **within** projects.
- `get_agent_provider_settings` is `Denied` deliberately — it exposes/configures provider
  process authority. Do not register it. Project it (see §3.2).
- Chat send is already solved via `send_remote_chat_message`; the spawn-free design doc
  (`docs/handoffs/remote-mobile/chat-send-spawn-free-design.md`) governs that seam.
- `detector (c)`: no registered command may reach a spawn sink. Any new registration must pass
  the capability detectors, not argue with them.

---

## 3. Required work

### 3.1 Shell read projections (backend, the unblocker)

Follow the two established precedents: the **`WorkerTaskView` projection** (project sensitive
rows to a safe DTO at the facade only) and the **remote twin** naming
(`list_remote_agent_conversations`).

New registered commands, all `Read` class, all hand-audited registry rows per rule 27:

| New command | Projects | Fields (allowlist, not the entity) |
|---|---|---|
| `list_remote_projects` | `Project` rows | id, name, description, created/updated, task counts, active-plan id, analysis status. **Never**: `working_directory`, git remotes, setup/validation commands, provider overrides |
| `get_remote_project` | one project | same projection |
| `get_remote_provider_readiness` | provider settings | `{ onboardingComplete: bool, enabledProviderCount: number }` — booleans/counts only; no provider identities, models, paths, or credentials |
| `get_remote_execution_status` | execution status | halt mode (`running/paused/stopped`), running counts — the pause-brake surface `ui:operate` already implies |

Each needs: `capability_ledger.rs` class row (compile-fails if over-privileged), registry
entry, parity test (byte-identical serialization vs a local call of the twin), scope-suite
row (allowed at `ui:read`, refused unauthenticated), and a P-11 census disposition for the
frontend name.

### 3.2 Event coverage for the projections

- `project:created` / `project:updated` / `project:deleted` backend events do not currently
  exist as classified names (only `project:analysis_*` do). Either add Durable events at the
  service seam (preferred — rule 28, classification-table row + emit via `AppState.events`)
  or accept poll-on-invalidate for v1.5 and record that in the classification table comments.
- CI rule already enforced: a frontend consumer of an unclassified durable name fails the
  event-manifest regenerate diff — so add the classification row in the same PR as the emit.

### 3.3 Frontend: remote-aware shell gating

- `useProjects()` (and the provider-readiness hook) route to the twins under a remote
  environment, same pattern as `chat.ts:1516` (`remoteTranscriptReadsEnabled()` branch).
- Welcome/onboarding gate (`App.tsx:346-350`): under a remote environment,
  `providerSetupRequired` must derive from `get_remote_provider_readiness` — never from local
  placeholder defaults; "no projects" may only render from a **successful** empty
  `list_remote_projects` answer. A failed/unavailable read renders the degraded
  "not available remotely / reconnect" shell, not first-run onboarding.
  `WelcomeScreen.tsx:46` already has a partial `isRemoteEnvironment` branch to extend.
- Surfaces whose commands stay unregistered (ticketing, GitHub connection, provider settings
  pane, project creation) must render the `agent-gate.ts` "runs only on the host" affordance
  (`REMOTE_COMMAND_UNAVAILABLE` mapping already exists at `agent-gate.ts:319`) instead of
  error or empty states. `canCreateProjects = !isRemoteEnvironment` (`App.tsx:247`) already
  models this — generalize the pattern rather than inventing a new one.

### 3.4 "Manage" scope for v1.5 (already mostly registered — wire the UI end-to-end)

In-scope, using existing registrations: task create/update, kanban moves that reduce to
registered `Operate` commands, pause/stop brakes, conversation reads + `send_remote_chat_message`
behind the per-device `ui:agent` toggle, automations/ideation/personas/notifications reads.
Out of scope (host-only, by owner decision): project create/update/delete, provider
onboarding, ticketing/GitHub integrations, terminal.

---

## 4. Invariants that bound this work (non-negotiable)

1. **Rule 27**: every new `:3849` command is a hand-audited `registry.rs` allowlist entry with
   a `capability_ledger.rs` class. No passthroughs, no `generate_handler!` edits, no forks.
2. **Rule 28**: new remote events go through the classification table; Durable rides the
   sequencer. Never re-derive from `EventSink`.
3. Projections are **allowlists of fields**, defined at the facade (`WorkerTaskView`
   precedent) — never `Entity::into()` of the full row.
4. Scope comes from `scope_for_class` only; capability detectors decide registrability.
5. The client treats `REMOTE_COMMAND_UNAVAILABLE` as a capability boundary everywhere
   (hydration barrier already does; §3.3 extends this to render-level gating).
6. Hydration stays inside the paced budget (`request-pacing.ts`); the new shell queries add
   ~4 calls, well within it.

---

## 5. Suggested lane split

| Lane | Contents | Test surface |
|---|---|---|
| A (backend) | §3.1 four projections + ledger + registry + parity/scope tests | `remote_e2e` leg: fresh client boots to a populated workspace mirror; scope suite; parity tables |
| B (frontend) | §3.3 gating + twin routing + host-only affordances | `App` boot-gate tests under mocked remote env; `WelcomeScreen` remote states; agent-gate copy tests |
| C (events) | §3.2 project lifecycle events + classification rows | event-manifest CI + sequencer delivery leg |
| D (polish) | Connections pane copy for partially-available hosts; docs (`remote-access.md` §surfaces table) | docs + Playwright visual pass on the remote shell |

A unblocks B; C can land in parallel; D last. The e2e acceptance for the whole handoff: pair a
fresh client against a host with ≥1 project, ≥1 conversation, ≥1 automation → the client
renders sidebar + kanban + agents list from host data within one hydrate, with host-only
surfaces visibly gated — and never shows first-run onboarding.
