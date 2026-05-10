---
paths:
  - "config/ralphx.yaml"
  - "agents/**"
  - "plugins/app/ralphx-mcp-server/src/**"
  - "src-tauri/src/infrastructure/agents/**"
  - "src-tauri/src/application/chat_service/**"
  - "src-tauri/src/commands/execution_commands.rs"
  - "src-tauri/src/commands/execution_commands/**"
  - "src-tauri/src/domain/entities/chat_conversation.rs"
  - "frontend/src/types/chat-conversation.ts"
  - "frontend/src/lib/chat-context-registry.ts"
  - "frontend/src/stores/chatStore.ts"
  - "frontend/src/hooks/useAgentEvents.ts"
  - "frontend/src/components/Chat/**"
---

# Agent Type Map

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

## Agent Context Types

Source of truth: `ChatContextType` (Rust: `domain/entities/chat_conversation.rs`) | `ContextType` (TS: `types/chat-conversation.ts`)

| Context Type | Store Key Prefix | Execution Slot | Streaming | Subagents | Diff Views | Queue | Team Mode | Pipeline Stage |
|---|---|---|---|---|---|---|---|---|
| `ideation` | `session:` | ✅ | ✅ | ✅ | ❌ | ✅ | ✅ | — |
| `task` | `task:` | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ | — |
| `project` | `project:` | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ | — |
| `task_execution` | `task_execution:` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | Executing → ReExecuting |
| `review` | `review:` | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ | PendingReview → Reviewing |
| `merge` | `merge:` | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ | PendingMerge → Merging |

**Execution slot** = counted against `max_concurrent` in `uses_execution_slot()` (`chat_service/mod.rs`).

## Named Agents (config/ralphx.yaml)

Canonical authoring flow: `.claude/rules/agent-authoring.md`

| Agent Name | Context | Model | Role |
|---|---|---|---|
| `ralphx-ideation` | ideation | opus | Lead for ideation sessions (proposals, plans). Key MCP: `create_child_session`, `get_plan_verification`, `stop_verification`, `revert_and_skip` |
| `ralphx-ideation-readonly` | ideation | opus | Read-only ideation fallback |
| `ralphx-ideation-team-lead` | ideation | opus | Team mode lead for ideation. Key MCP: `request_team_plan`, `create_child_session`, `get_plan_verification`, `stop_verification` |
| `ralphx-utility-session-namer` | ideation | haiku | Names ideation sessions |
| `ralphx-chat-task` | task | sonnet | Task-level Q&A |
| `ralphx-chat-project` | project | sonnet | Project-level Q&A |
| `ralphx-execution-worker` | task_execution | sonnet | Code execution, step management |
| `ralphx-execution-coder` | task_execution | sonnet | Alternative worker variant |
| `ralphx-execution-team-lead` | task_execution | sonnet | Team mode worker |
| `ralphx-execution-reviewer` | review | sonnet | Automated code reviewer |
| `ralphx-review-chat` | review | sonnet | Interactive review assistant |
| `ralphx-review-history` | review | sonnet | Historical review context |
| `ralphx-execution-merger` | merge | sonnet | Merge conflict resolution |
| `ralphx-research-deep-researcher` | — | opus | Deep research for ideation |
| `ralphx-execution-orchestrator` | — | opus | Internal orchestration |
| `ralphx-qa-prep` | — | sonnet | QA preparation |
| `ralphx-qa-executor` | — | sonnet | QA execution |
| `ralphx-project-analyzer` | — | haiku | Project analysis |
| `ralphx-memory-maintainer` | — | sonnet | Memory management |
| `ralphx-memory-capture` | — | sonnet | Memory capture |
| `plan-critic-layer1` | — | opus | Layer 1 completeness critic for plan verification. Returns structured JSON gap analysis only. |
| `plan-critic-layer2` | — | opus | Dual-lens implementation critic (minimal/surgical + defense-in-depth). Read-only. No Write/Edit/Bash. |
| `ralphx-plan-verifier` | — | opus | Owns adversarial round loop — spawns critics, merges gaps, revises plan. |
| `ralphx-ideation-specialist-backend` | — | opus | Research Rust/Tauri/SQLite patterns for ideation teams |
| `ralphx-ideation-specialist-frontend` | — | opus | Research React/TypeScript/Tailwind patterns for ideation teams |
| `ralphx-ideation-specialist-infra` | — | opus | Research database schema, MCP, config, and git patterns for ideation teams |
| `ralphx-ideation-specialist-ux` | — | opus | UX/flow verification specialist — ASCII wireframes, user flow diagrams, screen inventory, UX gap analysis. Tools: Read, Grep, Glob, WebFetch, WebSearch. MCP: `get_session_plan`, `get_artifact`, `create_team_artifact`, `get_team_artifacts`, `list_session_proposals`, `get_proposal`, `get_parent_session_context`, `search_memories`, `get_memory`, `get_memories_for_paths`. DisallowedTools: Write, Edit, NotebookEdit, Bash. Spawned by ralphx-plan-verifier during VERIFY and by ralphx-ideation-team-lead / ralphx-ideation during EXPLORE when UI/UX signals detected. |
| `ralphx-ideation-specialist-code-quality` | — | opus | Pre-round enrichment specialist — reads actual code paths referenced in the plan, identifies targeted quality improvements (complexity reduction, DRY violations, extract opportunities, naming). Runs ONCE before the adversarial loop begins (Step 0.5). Findings injected into plan context so critics see them in every round. Spawned by ralphx-plan-verifier unconditionally when plan references existing code files. |
| `ralphx-ideation-specialist-intent` | — | opus | Intent alignment specialist — compares plan goal against original user messages across 4 axes (scope, constraints, priorities, success criteria), flags narrowing/broadening/substitution mismatches. Runs ONCE before adversarial loop (Step 0.5). Tools: Read, Grep, Glob, WebFetch, WebSearch. MCP: `create_team_artifact`, `get_team_artifacts`, `get_session_plan`, `get_artifact`, `get_session_messages`, `search_memories`, `get_memory`, `get_memories_for_paths`. DisallowedTools: Write, Edit, NotebookEdit, Bash. Spawned by ralphx-plan-verifier unconditionally for ALL plans (no Affected Files gate). |
| `ralphx-ideation-specialist-prompt-quality` | — | opus | Per-round prompt quality specialist — token efficiency, information scoping, anti-bloat, tool-prompt alignment. Tools: Read, Grep, Glob, WebFetch, WebSearch. MCP: `create_team_artifact`, `get_team_artifacts`, `get_session_plan`, `get_artifact`. DisallowedTools: Write, Edit, NotebookEdit, Bash. Spawned by ralphx-plan-verifier when Affected Files contains `.md` files in `agents/` or `prompts/` directories. |
| `ralphx-ideation-specialist-pipeline-safety` | — | opus | Per-round pipeline safety specialist — cross-references proposed changes against 5 synthetic failure archetypes (merge worktree lifecycle, auto-transition churn, SQLite concurrent access, agent status desync, incomplete event coverage). Reads actual source files. Tools: Read, Grep, Glob, WebFetch, WebSearch. MCP: `create_team_artifact`, `get_team_artifacts`, `get_session_plan`, `get_artifact`. DisallowedTools: Write, Edit, NotebookEdit, Bash. Trigger: Affected Files contains any of `side_effects/`, `task_transition_service.rs`, `on_enter_states/`, `chat_service_merge.rs`, `chat_service_streaming.rs`. Dispatch mode: per-round parallel. |
| `ralphx-ideation-specialist-state-machine` | — | opus | Per-round state machine safety specialist — evaluates plans that modify task state transitions: checks on_enter handlers, concurrency guards, reconciler handling, rollback paths, single-fire guards. Tools: Read, Grep, Glob, WebFetch, WebSearch. MCP: `create_team_artifact`, `get_team_artifacts`, `get_session_plan`, `get_artifact`. DisallowedTools: Write, Edit, NotebookEdit, Bash. Trigger: Affected Files contains `task_transition_service.rs` or `on_enter_states/`; or plan adds new pipeline stages or auto-transitions. Dispatch mode: per-round parallel. |
| `ralphx-ideation-advocate` | — | opus | Advocate for a specific approach in architectural debates |
| `ralphx-ideation-critic` | — | opus | Stress-test all approaches with adversarial analysis |

**Memory tools:** Most agents also have memory read tools (`search_memories`, `get_memory`, `get_memories_for_paths`) — see `config/ralphx.yaml` for the authoritative `mcp_tools` list per agent.

## Verification Specialist Extensibility Pattern

Adding a new specialist to the plan verification pipeline requires these 7 steps:

| Step | File | Change |
|------|------|--------|
| 1 | `agents/<name>/...` | Create canonical agent config + prompt files with role/scope/refuse boundaries and output format |
| 2 | `config/ralphx.yaml` | Register agent: model, tools, mcp_tools, disallowedTools |
| 3 | `plugins/app/ralphx-mcp-server/src/agentNames.ts` | Add `export const IDEATION_SPECIALIST_<NAME> = "<name>"` constant |
| 4 | `plugins/app/ralphx-mcp-server/src/tools.ts` | Import constant; add `[IDEATION_SPECIALIST_<NAME>]: [...]` to TOOL_ALLOWLIST |
| 5 | `agents/ralphx-plan-verifier/claude/prompt.md` + `config/ralphx.yaml` | Keep the Claude prompt contract and runtime grants aligned for `Task(ralphx:<name>)` |
| 6 | `config/ralphx.yaml` ralphx-plan-verifier entry | Add `Task(ralphx:<name>)` to `preapproved_cli_tools` array |
| 7 | `agents/ralphx-plan-verifier/claude/prompt.md` | Add signal → specialist mapping in dynamic role selection section |

**Two specialist dispatch modes:**
| Mode | When | Example |
|------|------|---------|
| **Pre-round enrichment** (Step 0.5) | Runs ONCE before adversarial loop; results injected into plan context | `ralphx-ideation-specialist-code-quality` — unconditionally when plan references existing code files; `ralphx-ideation-specialist-intent` — unconditionally for ALL plans (no Affected Files gate) |
| **Per-round parallel** | Runs alongside critics in each round; selected by signal | `ralphx-ideation-specialist-ux` — `.tsx`/`.ts` in `src/`; `ralphx-ideation-specialist-prompt-quality` — `.md` in `agents/`/`prompts/`; `ralphx-ideation-specialist-pipeline-safety` — `side_effects/`/`task_transition_service.rs`/`on_enter_states/`/`chat_service_merge.rs`/`chat_service_streaming.rs`; `ralphx-ideation-specialist-state-machine` — `task_transition_service.rs`/`on_enter_states/` or new pipeline stages |

**Signal mapping rules (per-round specialists):** Scan `## Affected Files` and `## Architecture` sections only (not full plan text). Return: specialist name, trigger signal, signal source. Per-round specialists run in parallel with critics — failure is non-blocking. Specialists create artifacts on the **parent ideation session_id** (not the verification child session_id) so they appear in the Team Artifacts tab.

## Cross-Project Tool Chain

Full ordered sequence when an ideation session detects cross-project targets:

| Step | Tool | Purpose | Agents with Access |
|------|------|---------|-------------------|
| 1 | `cross_project_guide` | Detect if plan spans multiple projects; gate for all subsequent cross-project tools | `ralphx-ideation`, `ralphx-ideation-team-lead` |
| 2 | `list_projects` | Discover available target projects by ID/name | `ralphx-ideation`, `ralphx-ideation-team-lead` |
| 3 | `ask_user_question` | Confirm target project selection with user | `ralphx-ideation`, `ralphx-ideation-team-lead` |
| 4 | `create_cross_project_session` | Create a new ideation session in the target project (requires plan verification = Verified/Skipped/ImportedVerified) | `ralphx-ideation`, `ralphx-ideation-team-lead` |
| 5 | `create_task_proposal` (with `target_project`) | Create proposals in source session; set `target_project` field on each cross-project proposal | `ralphx-ideation`, `ralphx-ideation-team-lead` |
| 6 | `migrate_proposals` | Move cross-project proposals to their target session; call once per target session after proposals are created | `ralphx-ideation`, `ralphx-ideation-team-lead` |
| 7 | `finalize_proposals` (per session) | Finalize each session separately — source session first, then each target session | `ralphx-ideation`, `ralphx-ideation-team-lead` |

**Constraints:** `cross_project_guide` must be called before `create_task_proposal` when a cross-project plan exists (`cross_project_checked` gate). `create_cross_project_session` requires plan verification = Verified/Skipped/ImportedVerified.

## Agent Frontmatter MCP Tool Rule (NON-NEGOTIABLE)

Agent frontmatter `tools:` MUST use explicit `mcp__ralphx__<tool>` entries — ❌ `mcp__ralphx__*` wildcards. The `config/ralphx.yaml` `mcp_tools` array is the source of truth.

**Why:** Wildcard doesn't reliably resolve; agents get "tool doesn't exist" for valid tools. All three layers must include the tool: frontmatter → `config/ralphx.yaml` `mcp_tools` → MCP server TOOL_ALLOWLIST.

\`\`\`yaml
# ✅ Explicit entries in agent frontmatter
tools:
  - mcp__ralphx__get_task_context
  - mcp__ralphx__execution_complete

# ❌ Wildcard — unreliable resolution
tools:
  - mcp__ralphx__*
\`\`\`

**Spot-check (2026-03-20):** `ralphx-execution-worker` (23 tools ✅), `ralphx-execution-reviewer` (15 tools ✅), `ralphx-execution-merger` (9 tools ✅) — all frontmatter entries match `config/ralphx.yaml` exactly.

## Agent Lifecycle Events

All handled by `useAgentEvents` hook (`src/hooks/useAgentEvents.ts`).

| Event | Source | State Transition | Notes |
|---|---|---|---|
| `agent:run_started` | Agent process spawned | `idle` → `generating` | Sets active conversation |
| `agent:conversation_created` | Backend conversation creation | — (query invalidation) | Fires when new ChatConversation row is created. Frontend invalidates conversation list. |
| `agent:message_created` | Agent produces output | — (query invalidation) | Appends to message list |
| `agent:turn_completed` | Agent finishes one turn | `generating` → `waiting_for_input` | Agent alive, awaiting user input |
| `agent:run_completed` | Agent process exits | `*` → `idle` | Flushes queued messages |
| `agent:queue_sent` | Queued message delivered | — (removes from queue) | Matches by content hash |
| `agent:stopped` | User stops agent | `*` → `idle` | User-initiated stop |
| `agent:error` | Agent crash/error | `*` → `idle` | Toast for execution/review/merge |
| `agent:session_recovered` | Session recovery | — (info toast) | Notification only |

## Frontend State: AgentStatus (Tri-State)

Defined in `stores/chatStore.ts`. Keyed by store key (e.g., `"task_execution:task-123"`).

```
AgentStatus = "idle" | "generating" | "waiting_for_input"
```

| Status | Meaning | UI | Queue Behavior |
|---|---|---|---|
| `idle` | No agent process | Hidden / activity icon | Direct send |
| `generating` | Agent producing output | Spinner + "Agent responding..." | Queue message |
| `waiting_for_input` | Agent alive, between turns | Pause icon + "Awaiting input" | Direct send |

**Selectors:**
- `selectAgentStatus(key)` → `AgentStatus` (tri-state)
- `selectIsAgentRunning(key)` → `boolean` (true for both `generating` and `waiting_for_input` — backward compat)

## Store Key Construction

`buildStoreKey(contextType, contextId)` from `lib/chat-context-registry.ts`:

| Context Type | Key Format | Example |
|---|---|---|
| `ideation` | `session:{sessionId}` | `session:abc-123` |
| `task` | `task:{taskId}` | `task:task-456` |
| `project` | `project:{projectId}` | `project:proj-789` |
| `task_execution` | `task_execution:{taskId}` | `task_execution:task-456` |
| `review` | `review:{taskId}` | `review:task-456` |
| `merge` | `merge:{taskId}` | `merge:task-456` |
