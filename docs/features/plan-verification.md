# Verify Plan

Verify Plan is an optional model-native review pass for a Plan-mode artifact. It runs as a visible turn in the same planning conversation, using the active model and its normal delegation capabilities.

## Behavior

1. The user, automatic policy, or external MCP triggers `Verify Plan`.
2. RalphX queues a typed `verify_plan` turn in the active Plan conversation.
3. The active model rereads the current plan and relevant repository context, chooses useful review lenses, and may use its normally allowed delegates.
4. The model revises the existing linked plan when it finds actionable issues.
5. When satisfied, that same turn calls the bounded completion operation.
6. RalphX records proof for the exact final plan artifact.

There is no separate verifier agent, fixed critic roster, hidden verification session, round loop, gap ledger, or Verification tab. Review reasoning and plan revisions remain visible in the planning conversation.

## Settings

| Setting | Default | Effect |
|---|---:|---|
| Auto-verify plans | Off | Automatically queues Verify Plan after the authoritative plan artifact changes. |
| Require verification before acceptance | Off | Blocks acceptance until proof matches the current plan artifact. |

External sessions may override either setting. A null override inherits the global/project value. Being an external session does not itself force verification.

## Exact-version proof

Verification is satisfied only when the session's current `plan_artifact_id` equals its `verified_plan_artifact_id`. If review revises the plan, completion proves the revised artifact. If the plan changes later, the prior proof is stale and no longer satisfies a required acceptance gate.

Only the live matching `verify_plan` agent run can record proof. Ordinary planning turns, stale runs, wrong conversations, failed runs, and cancelled runs cannot authorize acceptance.

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
- Automatic trigger when `auto_verify_plans` is enabled.
- Internal MCP `get_plan_verification` and zero-argument `complete_plan_verification`.
- External MCP `v1_trigger_plan_verification` and `v1_get_plan_verification`.

The external trigger queues the same visible planning turn; it does not start a separate verifier runtime.

## Relationship to workspace review

Verify Plan reviews intent and implementation strategy before execution. Workspace Review evaluates the actual code after execution. The high-value delivery loop remains Plan -> Execute -> Workspace Review -> Revise -> Re-review.
