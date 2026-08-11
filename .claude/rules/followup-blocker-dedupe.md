---
paths:
  - "src-tauri/src/http_server/handlers/reviews/**/*.rs"
  - "src-tauri/src/http_server/handlers/session_linking/*.rs"
  - "src-tauri/src/http_server/helpers.rs"
  - "src-tauri/src/http_server/handlers/agent_issues.rs"
  - "src-tauri/src/infrastructure/sqlite/sqlite_ideation_session_repo.rs"
  - "src-tauri/src/infrastructure/sqlite/sqlite_agent_conversation_issue_repo.rs"
  - "src-tauri/src/infrastructure/sqlite/migrations/v20260328*_ideation_followup_provenance*.rs"
  - "src-tauri/src/infrastructure/sqlite/migrations/v20260329113000_ideation_blocker_fingerprint*.rs"
  - "src-tauri/src/infrastructure/sqlite/migrations/*agent_conversation_issue*.rs"
  - "src-tauri/crates/ralphx-domain/src/entities/agent_conversation_issue.rs"
  - "src-tauri/crates/ralphx-domain/src/entities/task_context.rs"
  - "src-tauri/crates/ralphx-domain/src/entities/ideation/mod.rs"
  - "agents/ralphx-execution-worker/**"
  - "agents/ralphx-execution-reviewer/**"
  - "plugins/app/ralphx-mcp-server/src/index.ts"
  - "plugins/app/ralphx-mcp-server/src/ideation-tools.ts"
  - "plugins/app/ralphx-mcp-server/src/tools.ts"
---

# Follow-up Blocker Dedupe

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

**Required Context:** task-execution-agents.md | orchestrator-ideation-workflows.md | agent-mcp-tools.md

---

## Goal

Prevent autonomous worker/reviewer flows from spawning duplicate follow-up ideation sessions for the same blocker.

---

## Current Model

| Rule | Detail |
|---|---|
| Accepted parent stays read-only | Follow-up work always spawns a child ideation session; never mutate accepted parent sessions |
| Provenance is first-class | Follow-up sessions store `source_task_id`, `source_context_type`, `source_context_id`, `spawn_reason`, `blocker_fingerprint` |
| Dedupe key is semantic, not wording | ✅ `blocker_fingerprint` | ❌ `spawn_reason` text | ❌ title text |
| Current automatic fingerprint scope | Only the out-of-scope drift blocker path auto-derives a stable fingerprint |
| Agent visibility | `get_task_context` returns `followup_sessions[]` plus `out_of_scope_blocker_fingerprint` |
| Tool resolution | `create_followup_session(source_task_id=...)` resolves both the local parent ideation session and the current out-of-scope blocker fingerprint automatically |
| Backend idempotency | `create_child_session` reuses an existing active child when `parent_session_id + source_task_id + blocker_fingerprint` match |
| Review reuse | Review exhausted-drift auto-follow-up reuses by `blocker_fingerprint` first; old `spawn_reason` matching is fallback-only when no fingerprint exists |

---

## Agent Conversation Issues

| Rule | Detail |
|---|---|
| Backend owns canonical identity | `register_agent_issue` computes `canonical_fingerprint`; agents provide evidence and optional legacy `blocker_fingerprint` |
| Occurrences preserve repeats | Duplicate reports append `agent_conversation_issue_occurrences`; do not overwrite the only evidence trail |
| Known issue classes | frontend dependency/PATH setup, MCP package-lock drift, Rails test-database setup, clippy baseline, runtime-index prerequisite, merge hook environment, task-scoped scope drift |
| Unknown fallback | Use raw `blocker_fingerprint` when present; avoid title-only semantic merging |
| Ambiguous candidates | Tool returns `needs_issue_disambiguation`; retry with `attach_to_issue_id` or `confirm_new + new_issue_reason + issue_check_token` |

---

## Why `spawn_reason` Is Not Enough

| Weak field | Why it fails |
|---|---|
| `spawn_reason` | wording can drift (`out_of_scope_failure` | `worker_blocker_followup` | future variants) |
| `source_context_type` | worker and reviewer can target the same blocker from different contexts |
| title / prompt text | too unstable; changes with phrasing |

**Rule:** If two autonomous flows should converge on the same blocker, they need a stable first-class fingerprint, not matching prose.

---

## Current Fingerprint Semantics

| Field | Meaning |
|---|---|
| `out_of_scope_blocker_fingerprint` on `TaskContext` | Stable blocker ID for the current task's out-of-scope drift |
| `followup_sessions[].blocker_fingerprint` | Stable blocker ID already attached to existing follow-up sessions |

For current scope-drift flow, fingerprint is derived from:
- `task.id`
- normalized `out_of_scope_files`

So:
- same task + same out-of-scope blocker → same fingerprint
- same task + different blocker surfaces → different fingerprint

---

## Agent Rules

| Agent | Rule |
|---|---|
| worker | Check `followup_sessions` in `get_task_context` before spawning a blocker follow-up |
| reviewer | Same; do not create another follow-up if the blocker already has one underway |
| worker/reviewer | In task/review flows, pass `source_task_id`; let the MCP tool resolve parent session + fingerprint |
| all | ❌ Do not guess parent session from imported/master ancestry |

---

## When To Extend This System

Add another blocker fingerprint type only when a real duplicate pattern appears in production or tests.

Examples that do not automatically have first-class fingerprints yet:
- cross-project blocker discovered during execution
- research blocker that should spawn more research
- merge/conflict blocker that should spin out separate work

---

## Extension Checklist

When a new blocker class needs dedupe:

1. Add a first-class fingerprint source
   - task context | review context | merge context | research context
2. Persist it on follow-up ideation sessions
   - `blocker_fingerprint`
3. Make the MCP/backend creation path resolve or accept it
4. Make backend reuse key off it
5. Add targeted tests:
   - worker-created follow-up
   - later reviewer/other-context follow-up
   - same blocker reuses
   - different blocker does not reuse

**Rule:** Do not generalize blocker identity preemptively. Add the next fingerprint only after a concrete duplicate pattern is observed.

---

## Key Files

| Component | Path |
|---|---|
| Task context fingerprint + existing follow-ups | `src-tauri/src/http_server/helpers.rs` |
| Review exhausted-drift auto-follow-up | `src-tauri/src/http_server/handlers/reviews/complete.rs` |
| Agent conversation issue registration | `src-tauri/src/http_server/handlers/agent_issues.rs` |
| Agent issue identity + occurrences | `src-tauri/crates/ralphx-domain/src/entities/agent_conversation_issue.rs` |
| Agent issue persistence | `src-tauri/src/infrastructure/sqlite/sqlite_agent_conversation_issue_repo.rs` |
| Child-session idempotent reuse | `src-tauri/src/http_server/handlers/session_linking/create.rs` |
| Follow-up provenance + fingerprint fields | `src-tauri/crates/ralphx-domain/src/entities/ideation/mod.rs` |
| Follow-up persistence | `src-tauri/src/infrastructure/sqlite/sqlite_ideation_session_repo.rs` |
| Worker/reviewer guidance | `agents/ralphx-execution-worker/claude/prompt.md` | `agents/ralphx-execution-reviewer/claude/prompt.md` |
| MCP follow-up/issue tools | `plugins/app/ralphx-mcp-server/src/index.ts`, `plugins/app/ralphx-mcp-server/src/tools.ts`, `plugins/app/ralphx-mcp-server/src/ideation-tools.ts` |
