---
paths:
  - "agents/ralphx-ideation/**"
  - "agents/ralphx-ideation-readonly/**"
  - "src-tauri/src/infrastructure/agents/**"
  - "src-tauri/src/application/chat_service/**"
  - "plugins/app/ralphx-mcp-server/src/**"
---

> **Maintainer note:** Keep this file compact. Prefer one-line rules, links to source docs, and explicit non-negotiables over prose.

# Ideation Conversation Workflows

Canonical prompt and capability contracts live under `agents/ralphx-ideation*`; this rule records cross-surface invariants only.

## Profiles

| Surface | Contract |
|---|---|
| Active ideation | May maintain the plan and proposals using its canonical tool surface. |
| Accepted ideation | Read-only; mutations require a child session. |
| Agent conversation Plan profile | Read-only filesystem/research plus linked plan-artifact maintenance; no proposal or implementation pipeline. |

## Durable Flow

1. Recover the current plan/proposals/parent context through tools available to the active profile.
2. Clarify material ambiguities and inspect evidence before changing the plan.
3. Maintain one linked plan artifact; create it before proposals when the proposal workflow is active.
4. Use current proposal tools: `create_task_proposal`, `update_task_proposal`, archive/delete helpers, dependency analysis, then `finalize_proposals`.
5. Never mutate an accepted session; use `create_child_session` with the user request as `initial_prompt`.
6. Plan verification uses the backend-owned action/state flow; call only the completion tool exposed to the live verifier action and do not replay backend bookkeeping.

## Source Rules

- Prompt prose must name only tools available to that exact agent/profile.
- `agents/<agent>/agent.yaml` owns capabilities and delegation allowlists; `config/ralphx.yaml` is not the agent catalog.
- Backend owns dispatch, waits, settlement, parent routing, persisted verification state, and terminal classification.
- Current architecture: `.claude/rules/ideation-verification-architecture.md` | tool alignment: `.claude/rules/agent-mcp-tools.md`.
