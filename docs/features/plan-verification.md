# Verify Plan

Verify Plan is an optional model-native review pass for a Plan-mode artifact. It runs as a visible turn in the same planning conversation, using the active model and its normal delegation capabilities.

## Behavior

1. A successful ordinary Plan turn, the user, a verification-gated acceptance attempt, or external MCP triggers `Verify Plan`.
2. RalphX queues a typed `verify_plan` turn in the active Plan conversation.
3. The active model rereads the current plan and relevant repository context, chooses useful review lenses, and may use its normally allowed delegates.
4. The model revises the existing linked plan when it finds actionable issues.
5. When satisfied, that same turn calls the bounded completion operation.
6. RalphX records proof for the exact final plan artifact.

There is no separate verifier agent, fixed critic roster, hidden verification session, round loop, gap ledger, or Verification tab. Review reasoning and plan revisions remain visible in the planning conversation.

## Settings

| Setting | Default | Effect |
|---|---:|---|
| Verify draft plans automatically | On | After a successful ordinary Plan turn publishes the current draft, queues one visible Verify Plan turn. |
| Queue missing verification on acceptance | Off | Legacy fallback: when verification is required, the first unverified acceptance attempt queues Verify Plan and remains blocked until proof is recorded. |
| Require verification before acceptance | Off | Blocks acceptance until proof matches the current plan artifact. |

External sessions may override either setting. A null override inherits the global/project value. Being an external session does not itself force verification.

Automatic draft verification is advisory and independent from the acceptance gate. It is admitted only after the latest ordinary Plan turn has durably saved its assistant output and successfully completed its own run. Failed persistence, stale or superseded completions, non-Plan workspaces, and verifier turns do not trigger it. Duplicate admissions converge on the existing queued or running verifier.

The typed verifier always starts as a fresh process. An idle interactive Claude process is retired rather than receiving verifier instructions over stdin, while its provider session id may still be used for model continuity.

## Exact-version proof

Verification is satisfied only when the session's current `plan_artifact_id` equals its `verified_plan_artifact_id`. If review revises the plan, completion proves the revised artifact. If the plan changes later, the prior proof is stale and no longer satisfies a required acceptance gate.

Only the live matching `verify_plan` agent run can record proof. Ordinary planning turns, stale runs, wrong conversations, failed runs, and cancelled runs cannot authorize acceptance.

## Approval notification timing

When automatic draft verification is pending, RalphX stores an exact-artifact deferred marker instead of recording the durable Plan Approval notification. This suppresses the in-app toast, Attention item, notification history row, and desktop notification together. Verifier success or failure releases the current artifact exactly once; revising the plan replaces the marker, and startup reconciliation releases markers stranded after a restart only when no verifier remains queued or running.

## Statuses

| Status | Meaning |
|---|---|
| `unverified` | Current artifact has no matching proof. |
| `queued` | The typed review turn is waiting to run. |
| `verifying` | The typed review turn is running. |
| `verified` | Proof matches the current artifact. |
| `failed` | The authoritative review turn failed. |
| `cancelled` | The authoritative review turn was cancelled. |

## Entry points

- Plan-mode `Verify Plan` CTA.
- Successful completion of the latest ordinary Plan turn when `auto_verify_draft_plans` is enabled.
- Required acceptance with `auto_verify_plans` enabled.
- Internal MCP `get_plan_verification` and zero-argument `complete_plan_verification`.
- External MCP `v1_trigger_plan_verification` and `v1_get_plan_verification`.

The external trigger queues the same visible planning turn; it does not start a separate verifier runtime.

Both Plan controls remain visible after proof is recorded and show `Verified` with success styling. Selecting that state asks for confirmation before queuing an explicit rerun; the existing exact-artifact proof remains valid unless the plan changes.

## Relationship to workspace review

Verify Plan reviews intent and implementation strategy before execution. Workspace Review evaluates the actual code after execution. The high-value delivery loop remains Plan -> Execute -> Workspace Review -> Revise -> Re-review.
