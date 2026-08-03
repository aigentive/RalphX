---
paths:
  - "src-tauri/src/application/chat_service/**"
  - "src-tauri/src/application/reconciliation/**"
  - "src-tauri/src/application/task_transition_service.rs"
  - "src-tauri/src/domain/state_machine/**"
  - "src-tauri/src/http_server/handlers/steps.rs"
  - "src-tauri/src/http_server/types.rs"
  - "agents/ralphx-execution-*/**"
  - "docs/handoffs/**"
---

> **Maintainer note:** Keep this file compact. Prefer one-line rules, links to source docs, and explicit non-negotiables over prose.

# Stateful Workflow Review

Source note: `docs/development/llm-review-failure-patterns.md`

| Rule | Detail |
|---|---|
| False-success review required | For completion/cache/retry/recovery/state-machine fixes, run a post-implementation adversarial pass that tries to prove the task can advance, emit completion, or trust cached proof without current-attempt authority. |
| Attempt-scoped proof | Completion, validation cache, retry, resume, and finalizer decisions must be tied to the current run/attempt or latest status entry; commit SHA alone is not current-run proof. |
| Fail closed on reads | Repository/query/tool errors must not collapse into "no data" when "no data" permits forward progress; use explicit tri-state/typed errors. |
| Authority before effects | Emit completion events, webhooks, terminal metadata, and auto-commit only after final backend enforcement has accepted the transition. |
| Setup readiness proof | Backend setup-before-spawn must prove required setup succeeded or intentionally warn; setup failures must not masquerade as agent failures. |
| Sink-local path validation | DB/config/env/request/agent metadata/repo paths must be contained at filesystem/process sinks, even if they came from canonical config or existing task rows. |
| Prompt as API client | Prompt examples and tool instructions must match the live MCP surface, backend request schema, CWD/session semantics, and harness-specific behavior. |
| Test falsification | Tests for workflow fixes must exercise production entry paths, seed realistic run/status history, include stale/duplicate/re-entry scenarios, and assert absence of bad metadata/events, not only final status. |

## Review Prompt

```text
Review this diff adversarially. Do not confirm the patch intent first.

Find false-success paths: cases where the system advances, emits completion,
or trusts cached proof without current-attempt authority.

For every state transition or completion decision, answer:
1. What exact evidence authorizes it?
2. Is that evidence scoped to the current run/attempt?
3. What happens if the repo/query/cache read errors?
4. What side effects happen before final state authority?
5. Could stale worker output, stale cache, or retry/re-entry reuse this path?

Report only concrete counterexamples with file:line refs.
```
