# Agent Naming Normalization

## Goal

Normalize all canonical agent names to one explicit namespace:

- `ralphx-<domain>-<purpose>`

This removes the current mixed inventory (`orchestrator-ideation`, `chat-task`, `memory-capture`, `ralphx-worker`, `project-analyzer`, etc.) and makes agent purpose obvious from the name alone.

## Naming Rules

- All canonical agent names start with `ralphx-`
- The second segment is the functional domain
- The remaining segments describe the concrete purpose
- Read-only variants append `-readonly`
- Specialist families keep a stable family prefix plus the specialization suffix
- Runtime aliases may be kept temporarily during migration, but canonical config and generated assets should converge on the normalized names

## Domain Prefixes

- `ralphx-ideation-*`
- `ralphx-execution-*`
- `ralphx-review-*`
- `ralphx-chat-*`
- `ralphx-memory-*`
- `ralphx-qa-*`
- `ralphx-project-*`
- `ralphx-plan-*`
- `ralphx-research-*`
- `ralphx-utility-*`

## Proposed Mapping

| Current | Target |
|---|---|
| `orchestrator-ideation` | `ralphx-ideation` |
| `orchestrator-ideation-readonly` | `ralphx-ideation-readonly` |
| `ideation-advocate` | `ralphx-ideation-advocate` |
| `ideation-critic` | `ralphx-ideation-critic` |
| `ideation-specialist-backend` | `ralphx-ideation-specialist-backend` |
| `ideation-specialist-frontend` | `ralphx-ideation-specialist-frontend` |
| `ideation-specialist-infra` | `ralphx-ideation-specialist-infra` |
| `chat-task` | `ralphx-chat-task` |
| `chat-project` | `ralphx-chat-project` |
| `ralphx-worker` | `ralphx-execution-worker` |
| `ralphx-coder` | `ralphx-execution-coder` |
| `ralphx-reviewer` | `ralphx-execution-reviewer` |
| `ralphx-review-chat` | `ralphx-review-chat` |
| `ralphx-review-history` | `ralphx-review-history` |
| `ralphx-merger` | `ralphx-execution-merger` |
| `ralphx-orchestrator` | `ralphx-execution-orchestrator` |
| `ralphx-qa-prep` | `ralphx-qa-prep` |
| `ralphx-qa-executor` | `ralphx-qa-executor` |
| `ralphx-deep-researcher` | `ralphx-research-deep-researcher` |
| `project-analyzer` | `ralphx-project-analyzer` |
| `memory-capture` | `ralphx-memory-capture` |
| `memory-maintainer` | `ralphx-memory-maintainer` |
| `session-namer` | `ralphx-utility-session-namer` |

## Migration Notes

- Keep compatibility aliases during the migration window for persisted rows, runtime lookups, and generated Claude/Codex artifacts
- Rename cohorts should be ordered by blast radius:
  1. utility + chat + memory + analyzer agents
  2. ideation + plan verification agents
  3. execution + review + QA agents
- Every cohort must update:
  - `agents/*`
  - `ralphx.yaml`
  - runtime name/role mappers
  - Claude generated assets
  - Codex generation/runtime surfaces
  - MCP allowlists
  - docs/tests

## Open Design Choice

The current proposal keeps the automated code reviewer in the execution family:

- `ralphx-worker` -> `ralphx-execution-worker`
- `ralphx-coder` -> `ralphx-execution-coder`
- `ralphx-reviewer` -> `ralphx-execution-reviewer`
- `ralphx-merger` -> `ralphx-execution-merger`

while the user-facing review discussion/history agents stay under `ralphx-review-*`.
