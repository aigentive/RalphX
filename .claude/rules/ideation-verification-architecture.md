---
paths:
  - "agents/ralphx-ideation/**"
  - "frontend/src/api/verification.ts"
  - "frontend/src/hooks/useVerificationStatus.ts"
  - "frontend/src/components/agents/**"
  - "plugins/app/ralphx-mcp-server/src/plan-tools.ts"
  - "plugins/app/ralphx-external-mcp/src/tools/ideation.ts"
  - "src-tauri/src/application/plan_verification_service.rs"
  - "src-tauri/src/domain/services/verification_gate.rs"
  - "src-tauri/src/http_server/handlers/verification/**"
  - "src-tauri/src/http_server/handlers/external/ideation_runtime/verification.rs"
  - "src-tauri/src/infrastructure/sqlite/migrations/*plan_verification*"
  - "docs/features/plan-verification.md"
  - "docs/external-mcp/**"
---

# Model-Native Plan Verification

## Native supervised task-pipeline authorization

- Native Plan approval authorizes neither proposal application nor scheduling.
- Create Proposals records the exact conversation/session attachment and requests decomposition only.
- Start Tasks revalidates the exact current artifact approval and proposal graph before using the canonical apply/scheduler path.
- Tasks follow-up appends require the durable attachment plus an explicit source user message; retries must not create a second task.
- External-origin and Automation flows keep their existing typed authorization and are not governed by native Autopilot.

**Required context:** `stateful-workflow-review.md` | `agent-mcp-tools.md` | `multi-harness.md` | `docs/features/plan-verification.md`

## Non-negotiables

| Rule | Contract |
|---|---|
| One visible turn | `Verify Plan` queues an ordinary turn in the active Plan conversation; never create a hidden verification child. |
| Active model owns review | The current model selects review lenses and may use its normal allowed delegates; no fixed verifier, critic, or specialist roster. |
| Exact proof | Acceptance proof matches both current Overview and Blueprint ids to their verified pointers. Any one-sided revision needs new proof; legacy v1 remains overview-only. |
| Typed authority | Only a live `agent_runs.action_kind = verify_plan` run with matching session, artifact, and conversation may complete verification. |
| Backend derives identity | `complete_plan_verification` has no model-supplied session, artifact, run, status, generation, or timestamp arguments. |
| Fail closed | Missing settings/proof, malformed action metadata, ordinary chat turns, stale runs, and failed/cancelled runs never satisfy a required gate. |
| Queue semantics survive | Typed action metadata must survive ordinary message queues and durable capacity-deferred pending prompts. |
| One status projection | Status is derived from exact proof plus the typed action run/queue; do not add a second verification state machine. |
| Completion admission is exact | Automatic review is admitted only after the latest ordinary Plan turn persists its assistant output and completes its own run CAS successfully. |
| Fresh verifier process | A typed verifier turn never reuses an idle interactive Claude stdin process; provider session continuity may be retained. |
| Approval notification follows review | While automatic draft verification is pending, defer the exact artifact's durable Plan Approval notification and all derived presentations until the verifier settles. |

## Runtime flow

```text
CTA / auto policy / external MCP
  -> request_plan_verification(session, source)
  -> resolve active Plan conversation
  -> queue ordinary message with verify_plan(session, deterministic current bundle target) metadata
  -> active model reviews repository + both plan documents, chooses lenses/delegates, revises either if needed
  -> model calls zero-argument complete_plan_verification exactly once
  -> transport derives run + conversation
  -> SQLite CAS checks live matching run + exact current pair + owning conversation
  -> both verified artifact pointers and verified_plan_agent_run_id are stored
```

With `auto_verify_draft_plans` enabled, successful completion of the latest ordinary turn in an
active Plan workspace enters the same request path after assistant persistence and guarded run
completion. Admission is conversation/session scoped and idempotent. Failed persistence, stale or
superseded runs, non-Plan workspaces, and verifier turns do not trigger another review.

## Ownership map

| Concern | Authority |
|---|---|
| Shared request/status/completion | `src-tauri/src/application/plan_verification_service.rs` |
| Action identity | `AgentRunActionKind::VerifyPlan` and action context/target fields |
| Exact proof | `ideation_sessions.verified_plan_artifact_id` + `verified_plan_blueprint_artifact_id` + `verified_plan_agent_run_id` |
| Atomic proof write | `IdeationSessionRepository::complete_plan_verification` SQLite implementation |
| Manual trigger | `POST /api/verification/confirm` |
| Agent completion | `POST /api/plan-verification/complete` |
| Internal status | `GET /api/ideation/sessions/:id/verification` |
| External trigger/status | `/api/external/trigger_verification`, `/api/external/plan_verification/:id` |
| Internal MCP | `plugins/app/ralphx-mcp-server/src/plan-tools.ts` |
| External MCP | `plugins/app/ralphx-external-mcp/src/tools/ideation.ts` |
| Product CTA/settings | Agent Plan surfaces + `IdeationSettingsPanel.tsx` |
| Automatic admission | Ordinary chat turn finalizers + `admit_automatic_plan_verification` |
| Interactive ownership | `InteractiveProcessRegistry` active/idle token and idle retirement |
| Deferred approval presentation | `plan_approval_notification_service.rs` + durable exact-artifact marker |

## Policy

| Setting | Default | Meaning |
|---|---:|---|
| `auto_verify_draft_plans` | `true` | After a successful ordinary Plan turn publishes the current draft, queue one visible Verify Plan turn automatically. |
| `auto_verify_plans` | `false` | Legacy acceptance fallback: when verification is required, queue Verify Plan on an unverified acceptance attempt. |
| `require_verification_for_accept` | `false` | Reject acceptance unless proof matches the current artifact. |
| External overrides | `null` | Inherit the base setting; session origin alone never forces verification. |

`require_verification_for_proposals` is obsolete: proposals and plan edits are not verification-gated.

## False-success tests

- Ordinary chat run cannot write proof.
- Wrong session/artifact/conversation cannot write proof.
- Revised artifact invalidates earlier proof by id mismatch.
- Failed/cancelled authoritative run clears or cannot establish proof.
- Queued and capacity-deferred turns preserve typed metadata.
- Duplicate manual/auto/external requests return already queued/running/verified.
- Automatic admission rejects failed persistence, stale run CAS, non-latest runs, and non-Plan workspaces.
- Automatic verifier settlement releases exactly one current-artifact Plan Approval notification on success or failure.
- Startup reconciliation releases stranded deferred markers only after no verifier remains queued/running.
- Required acceptance fails closed; advisory verification does not block.

## Removed surfaces

Do not reintroduce fixed verifier/critic/specialist agents, hidden verification sessions, round/gap/convergence orchestration, preflight confirmation for a first verification, a Verification artifact tab, verification-specific chat widgets, or revert/skip/stop controls. A manual rerun of an already verified artifact does require confirmation. Startup reconciliation is limited to durable deferred Plan Approval presentation; it must not recreate verifier children or runs.

## Validation

- Rust: focused gate, action-run, pending-drain, SQLite CAS, HTTP, and migration tests.
- Frontend: CTA, settings, and tab-availability tests plus `npm run typecheck`.
- MCP: build and test both `ralphx-mcp-server` and `ralphx-external-mcp` after source changes.
- Run `python3 scripts/validate_sqlite_migrations.py` and a legacy-reference scan before handoff.
