# RX-Native Team Mode

RX-native Team mode provides a standing coordinator with durable members, exact run bindings, task reservations, message delivery, wake batching, derived Team usage, and staged Team-to-Solo exits.

The capability flag remains **off by default** (`agent_conversation_team=false`). It must stay disabled until the mixed Claude/Codex runtime proof, recovery checks, and rollout review are accepted.

When enabled, the Team panel exposes roster and assignment controls, derived token/cost usage, and two confirmed exits:

- **Suspend Team** fences new dispatch and wakes, suspends idle members, and returns the conversation to Solo only after the durable exit stage completes.
- **Drain and close** fences new work, cancels active Team bindings and releases their reservations, closes the Team, then returns the conversation to Solo.

Switching Capabilities away from Team uses the same staged exit: it resumes a pending action or drains and closes, and the capability change fails if that exit cannot complete.

Usage is always derived from `TeamRunBinding` rows joined to `AgentRun`; there is no writable Team usage total. Startup recovery releases a workspace reservation when its exact Team binding is terminal.

Remaining follow-ups: controlled rollout telemetry, production exercise of mixed-provider launches, and an operator runbook for retrying a pending exit after a process crash.
