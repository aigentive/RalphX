# RX-Native Team Mode

RX-native Team mode provides a standing coordinator with durable members, exact run bindings, task reservations, message delivery, wake batching, derived Team usage, and staged Team-to-Solo exits.

The capability flag remains **off by default** (`agent_conversation_team=false`). It must stay disabled until the mixed Claude/Codex runtime proof, recovery checks, and rollout review are accepted.

When enabled, the Team panel exposes roster and assignment controls, derived token/cost usage, and two confirmed exits:

- **Suspend Team** fences new dispatch and wakes, suspends idle members, and returns the conversation to Solo only after the durable exit stage completes.
- **Drain and close** fences new work, cancels active Team bindings and releases their reservations, closes the Team, then returns the conversation to Solo.

Switching Capabilities away from Team uses the same staged exit: it resumes a pending action or drains and closes, and the capability change fails if that exit cannot complete. A conversation already in Team mode cannot leave through an ordinary send — leaving requires the capability-change action, which stages the Team exit above; a mid-conversation send that tries to flip capabilities out of Team is rejected instead of silently dropping the team.

Usage is always derived from `TeamRunBinding` rows joined to `AgentRun`; there is no writable Team usage total. Startup recovery releases a workspace reservation when its exact Team binding is terminal.

## Coordinator Tool Surface

The coordinator profile exposes a small, model-facing tool set; three of them read roster/liveness state and are easy to confuse:

| Tool | Use it to |
|---|---|
| `team_list` | Find idle assignment targets before calling `team_assign` |
| `team_roster` | See who exists — name, role, status, and whether the caller has coordinator or member authority |
| `team_status` | See if anyone is stuck — coordinator-only liveness join (running state, last activity, latest run), capped at 32 entries, a member with no delegated session degrades to a null `agent_state` instead of failing the read |

## Provider Pre-Flight

`team_add_member` checks that the resolved harness's provider is enabled in Settings before the member is admitted to the roster, using the same default-harness resolution as assignment. A disabled provider fails the add with a 409 pointing at Settings > Harness > Providers, instead of admitting an unusable member and only discovering the problem on first assignment.

## Completion Signal And Coordinator Wake

When a member assignment settles, the coordinator learns about it without polling:

1. Member assignment reaches a terminal status and the member returns to idle.
2. The backend sends a System-authored message to the coordinator (deterministic idempotency key keyed on the assignment, so a replayed settlement cannot double-wake).
3. The message queues a durable wake batch for the coordinator.
4. The wake dispatcher claims the batch and, if the coordinator is idle, sends a hidden resume-in-place turn carrying the pending Team messages; if the coordinator is already mid-turn, the batch is cancelled because the message is already sitting in the coordinator's durable queue and drains at the coordinator's next turn boundary.

Settlement itself never depends on this notification — a failed or skipped wake never blocks the member assignment from completing, and dispatch is fired in the background rather than awaited by the settlement path.

Automatic wakes are budget-limited: only wake bindings that are still live (planned, launching, or running) count against a Team's automatic-wake limit, so the budget recovers as batches finish instead of decaying to zero forever. Exhausting the budget emits a needs-attention event so a stalled Team is visible instead of silently stuck.

Remaining follow-ups: controlled rollout telemetry, production exercise of mixed-provider launches, and an operator runbook for retrying a pending exit after a process crash.
