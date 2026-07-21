# Ideation Pipeline Architecture

## Native supervised pipeline

Native Agent conversations use the explicit progression
`Plan → Approve Plan → Create Proposals → Tasks → Start Tasks → execution`.
`Create Proposals` records a durable conversation/session attachment and asks
for decomposition only; it creates no Kanban tasks or scheduler work. `Start
Tasks` revalidates the exact current approval and proposal graph before using
the canonical apply and scheduling path.

`Tasks` is contextual and returnable only for the owning conversation. Its
agent may append one explicit user-requested follow-up to the same accepted,
still-open plan and PR through the scoped append path. External MCP ideation
remains autonomous and is not gated by the native Autopilot flag.

`Autopilot` is a separate, default-off native capability. It does not rename
Automation mode or change external MCP authorization.

> **Source:** Derived from code audit of `src-tauri/src/commands/ideation_commands/`,
> `src-tauri/src/http_server/handlers/ideation.rs`, `artifacts.rs`, `external.rs`.
> Do not update from docs — re-audit the code if this becomes stale.

## Overview

The external MCP and Ideation Studio pipeline converts a conversation with an
orchestrator agent into scheduled tasks. It spans 7 stages from session
creation through acceptance cascade. Most stages are autonomous
(agent-driven); only proposal finalization requires an explicit agent or user
action. Native Agent conversations use the supervised progression above and
cannot use `finalize_proposals` while attached in Tasks mode.

```
Session Created
      │
      ├─ (optional) Session Naming ─────────────────── [background, cosmetic]
      │
      ▼
Plan Created (artifacts.rs)
      │
      ├─ Ordinary Plan turn persists + completes ──── [default-on auto admission]
      │                                      │
      │                                      └─ queue fresh visible Verify Plan turn
      │                                             └─ exact proof or terminal failure
      │
      ▼
◉ BREAKPOINT — finalize_proposals() (ideation.rs:134)
      │    Requires explicit agent call to advance
      │
      ├─ [required + unverified + auto] Queue visible Verify Plan turn
      │                                      │
      │                                      └─ record exact proof, retry acceptance
      │
      ▼
Proposals Applied → Tasks Created
      │
      ▼
Session Accepted → execution:queue_changed
```

---

## Stage 1: Session Creation

Four independent creation paths, all emitting the same 3-layer event.

| Path | Entry Point | File | Notes |
|------|-------------|------|-------|
| Tauri command | `create_ideation_session_impl()` | `ideation_commands_session.rs:31` | Standard UI path |
| Cross-project | `create_cross_project_session_impl()` | `ideation_commands_cross_project.rs:40` | Inherits verified plan from source; validates no circular imports |
| Import | `import_ideation_session()` | `ideation_commands_export.rs:43` | Also emits `ideation:session_imported` (Tauri only) |
| External HTTP | `start_ideation_http()` | `external.rs:750` | POST `/api/external/ideation/start_ideation`; optionally spawns ralphx-ideation agent |

**Event emitted:** `IdeationSessionCreated` — 3-layer (Tauri + external_events + webhook)

**Payload:**
```json
{ "sessionId": "string", "projectId": "string" }
```

**Agent involved:** None (creation is direct). Optional: `ralphx-ideation` spawned by external path.

**Autonomous:** Yes — all paths are fire-and-forget.

---

## Stage 2: Session Naming (Optional)

- **Trigger:** Explicit call from frontend or orchestrator via `spawn_session_namer()` — `ideation_commands_session.rs:515`
- **Agent:** `ralphx-utility-session-namer` (Haiku, 60s timeout)
- **Mechanism:** `tokio::spawn` + `update_session_title` MCP tool call on completion
- **Event:** `ideation:session_title_updated` — Layer 1 (Tauri) only; not wired to external_events/webhooks
- **Autonomous:** Background agent, optional, no system cascade on failure

---

## Stage 3: Plan Creation

- **Handler:** `create_plan_artifact()` — `artifacts.rs:200`
- **HTTP:** POST `/api/ideation/artifacts` (MCP tool: `create_plan_artifact`)
- **Agent:** the active Plan-mode conversation model

**Events emitted:**
1. `plan_artifact:created` — Layer 1 (Tauri) only; carries full artifact object for frontend UI
2. `IdeationPlanCreated` — 3-layer (Tauri + external_events + webhook)

**Payload for `IdeationPlanCreated`:**
```json
{
  "session_id": "string",
  "project_id": "string",
  "artifact_id": "string",
  "plan_title": "string",
  "timestamp": "RFC3339"
}
```

**Automatic draft verification:** With the default-on draft policy, the latest successful ordinary
Plan turn queues a typed `verify_plan` turn in the same conversation after assistant persistence and
guarded run completion. Admission rejects stale or failed turns and is idempotent. The verifier gets
a fresh process even when an interactive Claude process is idle, while provider session continuity
may be retained. The separate default-off acceptance policy remains a compatibility fallback when
acceptance requires exact proof. Capacity-deferred turns retain their action metadata.

The Plan Approval notification is deferred at publication while automatic review is pending. A
durable exact-artifact marker suppresses toast, Attention, history, and desktop presentations until
the verifier settles; terminal and startup reconciliation then record the current notification once.

**Autonomous:** Yes.

---

## Stage 4: Plan Verification

- **Service:** `application/plan_verification_service.rs`
- **HTTP:** request/status endpoints plus the run-scoped `/api/plan-verification/complete` proof endpoint
- **Agent:** the active Plan-mode model in the existing conversation; it chooses any useful reasoning lenses or allowed general-purpose delegates

**State machine:**
```
Unverified → Queued → Verifying → Verified
                                → Failed
                                → Cancelled
```

Verification proof is exact-artifact scoped. `complete_plan_verification` succeeds only for the
currently running typed action whose session, conversation, and target artifact all match. A plan
edit changes the current artifact id and therefore invalidates the previous proof automatically.

**Payload for `IdeationVerified`:**
```json
{
  "session_id": "string",
  "project_id": "string",
  "convergence_reason": "zero_blocking | human_review | ..."
}
```

**Guards:**
- External sessions cannot skip verification
- Cannot update verification if session is Archived or Accepted

**Auto-propose cascade:** If `convergence_reason == "zero_blocking"` AND session origin is External,
triggers Stage 5 (Auto-Propose) automatically.

**Autonomous:** Yes (agent-driven multi-round loop).

---

## Stage 5: Auto-Propose (External Sessions Only)

- **Functions:** `auto_propose_for_external()` + `auto_propose_with_retry()` — `ideation.rs:961`
- **Trigger:** Automatic when verification reaches `Verified` + `zero_blocking` + session is External
- **Mechanism:** Sends `<auto-propose>` XML message to orchestrator agent; retries 3× (1s/2s/4s backoff)

**Events emitted:**
1. `IdeationAutoProposeSent` — 3-layer — fires on first successful message delivery
2. `IdeationAutoProposeFailed` — 3-layer — fires after all 3 retries exhausted

**Payloads:**
```json
// IdeationAutoProposeSent
{ "session_id": "string", "project_id": "string" }

// IdeationAutoProposeFailed
{ "session_id": "string", "project_id": "string", "error": "string" }
```

**Scope:** External sessions only. Internal sessions skip auto-propose and transition to "ready" state.

**Autonomous:** Yes — async background, non-fatal.

---

## Stage 6: Proposal Finalization ◉ BREAKPOINT

This stage describes external MCP and Ideation Studio orchestration. A native
Tasks conversation must use the typed **Start Tasks** command instead.

- **Handler:** `finalize_proposals()` — `ideation.rs:134`
- **Implementation:** `finalize_proposals_impl()` — `helpers.rs:753`
- **HTTP:** POST `/api/ideation/proposals/finalize` (MCP tool: `finalize_proposals`)
- **Agent:** `ralphx-ideation` calls this tool explicitly (breakpoint requiring agent action)

**Logic:**
1. Validate session is Active
2. Count active (non-archived) proposals
3. Validate count matches `expected_proposal_count` if set
4. Call `apply_proposals_core()` — creates Task entities from proposals
5. Transition session to Accepted if all proposals applied successfully

**Events emitted:**
1. `IdeationProposalsReady` — 3-layer — always emitted
2. `IdeationSessionAccepted` — 3-layer — only if session transitions to Accepted
3. `execution:queue_changed` — Layer 1 (Tauri) only — notifies task scheduler

**Payloads:**
```json
// IdeationProposalsReady
{
  "session_id": "string",
  "project_id": "string",
  "proposal_count": "integer"
}

// IdeationSessionAccepted
{
  "session_id": "string",
  "project_id": "string",
  "timestamp": "RFC3339"
}
```

**Autonomous:** No — explicit orchestrator agent call required.

---

## Stage 7: Acceptance Cascade (Automatic)

Once `finalize_proposals` completes and session transitions to Accepted:

1. Tasks become `ready`/`executing` — queued for execution
2. `execution:queue_changed` notifies the task scheduler
3. Session moves to "in_progress" group in UI (done when all tasks complete)
4. Plan branch tracking begins

**Autonomous:** Yes — automatic on finalize success.

---

## Event Matrix

| Event | Stage | Tauri | external_events | Webhooks | Trigger Condition |
|-------|-------|-------|-----------------|----------|-------------------|
| `ideation:session_created` | 1 | ✅ | ✅ | ✅ | All 4 creation paths |
| `ideation:session_imported` | 1 | ✅ | ❌ | ❌ | Import path only |
| `ideation:session_title_updated` | 2 | ✅ | ❌ | ❌ | Naming agent completes |
| `plan_artifact:created` | 3 | ✅ | ❌ | ❌ | Plan artifact created (UI update) |
| `ideation:plan_created` | 3 | ✅ | ✅ | ✅ | Plan artifact created |
| `ideation:verification_status_changed` | 4 | ✅ | ❌ | ❌ | Any verification status change |
| `ideation:verified` | 4 | ✅ | ✅ | ✅ | Status transitions to `Verified` only |
| `ideation:auto_propose_sent` | 5 | ✅ | ✅ | ✅ | First successful auto-propose delivery |
| `ideation:auto_propose_failed` | 5 | ✅ | ✅ | ✅ | All retries exhausted |
| `ideation:proposals_ready` | 6 | ✅ | ✅ | ✅ | finalize_proposals succeeds |
| `ideation:session_accepted` | 6 | ✅ | ✅ | ✅ | Session transitions to Accepted |
| `execution:queue_changed` | 7 | ✅ | ❌ | ❌ | Task queue size changes |

**EventType enum:** `crates/ralphx-domain/src/entities/event_type.rs`

---

## Agent State Machine

```
                    ┌─────────────────────────┐
                    │   ralphx-ideation        │
                    └───────────┬─────────────┘
                                │ creates session, proposes plan
                                ▼
                    ┌─────────────────────────┐
                    │   same Plan conversation │
                    │   typed verify_plan turn │
                    │   optional model-chosen  │
                    │   general delegates      │
                    └───────────┬─────────────┘
                                │ complete_plan_verification(Verified)
                                ▼
                    ┌─────────────────────────┐
                    │   ralphx-ideation  │
                    │   (resumes after verify) │
                    └───────────┬─────────────┘
                                │ finalize_proposals() [BREAKPOINT]
                                ▼
                    ┌─────────────────────────┐
                    │    ralphx-execution-worker /       │
                    │    ralphx-execution-coder          │  ← tasks now executing
                    └─────────────────────────┘
```

**Agent types involved (from `agent-type-map.md`):**

| Agent | Context Type | Role in Pipeline |
|-------|-------------|-----------------|
| `ralphx-ideation` | `ideation` | Creates session, drives plan creation, calls finalize_proposals |
| `ralphx-utility-session-namer` | `ideation` | Names session title via `update_session_title` MCP tool |
| Active Plan-mode model | existing conversation | Reviews and, when needed, revises the current linked plan |
| Model-selected general delegates | delegated conversations | Optional context-specific evidence or adversarial lenses |

---

## External MCP Tool Mapping

Tools available to external agents (e.g., ReefBot) via `ralphx-external-mcp` (`:3848`):

| Pipeline Action | MCP Tool | HTTP Handler |
|----------------|----------|-------------|
| Create ideation session | `v1_start_ideation` | POST `/api/external/ideation/start_ideation` |
| Get session status | `v1_get_ideation_status` | GET `/api/ideation/sessions/{id}/status` |
| Get plan | `v1_get_plan` | GET `/api/ideation/sessions/{id}/plan` |
| Get plan verification | `v1_get_plan_verification` | GET `/api/ideation/sessions/{id}/verification` |
| Send message to agent | `v1_send_ideation_message` | POST `/api/ideation/sessions/{id}/messages` |
| Accept plan and schedule | `v1_accept_plan_and_schedule` | POST `/api/ideation/sessions/{id}/accept` |
| Resume scheduling (idempotent) | `v1_resume_scheduling` | POST `/api/ideation/sessions/{id}/resume` |
| Poll recent events | `v1_get_recent_events` | GET `/api/external/events` |

**Polling alternative for verification progress:** Use `v1_get_plan_verification` to read the
typed action state and exact-artifact proof (`unverified`, `queued`, `verifying`, `verified`,
`failed`, or `cancelled`).

**Polling alternative for agent state:** Use `v1_get_ideation_status` to check
`agent_status` (idle/generating/waiting_for_input).

---

## External and Ideation Studio Autonomy Summary

| Stage | User/Agent Action Required | Notes |
|-------|---------------------------|-------|
| 1. Session Creation | User or external agent creates session | 4 paths |
| 2. Session Naming | None (optional, background) | Cosmetic only |
| 3. Plan Creation | Orchestrator agent creates plan | Agent-driven |
| 4. Plan Verification | None (agents run loop autonomously) | Multi-round, convergent |
| 5. Auto-Propose | None (external sessions only) | Automatic on `zero_blocking` |
| 6. **Proposal Finalization** | **Orchestrator agent must call `finalize_proposals`** | ◉ Only breakpoint |
| 7. Acceptance Cascade | None | Automatic on finalize success |

---

## Three-Layer Emission Pattern

All externally-visible ideation events follow the same non-fatal emission pattern:

```rust
// Layer 1: Frontend UI notification
app_handle.emit("ideation:event_name", &payload);

// Layer 2: Persist to external_events table (for v1_get_recent_events polling)
if let Err(e) = external_events_repo
    .insert_event("ideation:event_name", &project_id, &payload_str)
    .await
{
    tracing::warn!(error = %e, "Failed to persist ideation event");
}

// Layer 3: Outbound webhook delivery
if let Some(ref publisher) = webhook_publisher {
    let _ = publisher.publish(EventType::IdeationX, &project_id, payload_json).await;
}
```

**Non-fatal guarantee:** Layer 2 and Layer 3 failures use `tracing::warn` — they never block or
fail the ideation operation. An event delivery failure cannot interrupt the pipeline.

**AppState access pattern:**
- Tauri commands: `state.external_events_repo` + `state.webhook_publisher` (direct AppState)
- HTTP handlers: `state.app_state.external_events_repo` + `state.app_state.webhook_publisher`
  (via `State<HttpServerState>` → `.app_state`)

---

## Key Files

| Component | File |
|-----------|------|
| Session creation (Tauri) | `src-tauri/src/commands/ideation_commands/ideation_commands_session.rs` |
| Cross-project | `src-tauri/src/commands/ideation_commands/ideation_commands_cross_project.rs` |
| Import/export | `src-tauri/src/commands/ideation_commands/ideation_commands_export.rs` |
| External HTTP creation | `src-tauri/src/http_server/handlers/external.rs` |
| Plan artifacts | `src-tauri/src/http_server/handlers/artifacts.rs` |
| Verification + proposals | `src-tauri/src/http_server/handlers/ideation.rs` |
| Proposal application | `src-tauri/src/http_server/helpers.rs` |
| EventType enum | `src-tauri/crates/ralphx-domain/src/entities/event_type.rs` |
| External MCP tools | `plugins/app/ralphx-external-mcp/src/tools/` |
