---
paths:
  - "agents/**"
  - "config/**"
  - "src-tauri/crates/ralphx-domain/src/entities/chat_conversation.rs"
  - "src-tauri/src/application/chat_service/**"
  - "frontend/src/types/chat-conversation.ts"
  - "frontend/src/lib/chat-context-registry.ts"
  - "frontend/src/stores/chatStore.ts"
  - "frontend/src/hooks/useAgentEvents.ts"
  - "frontend/src/components/Chat/**"
---

> **Maintainer note:** Keep this file compact. Prefer one-line rules, links to source docs, and explicit non-negotiables over prose.

# Agent And Conversation Type Map

## Sources Of Truth

| Concern | Canonical source |
|---|---|
| Agent inventory, role, capabilities, harness metadata, delegation | `agents/<agent>/agent.yaml` |
| Harness prompt support | `agents/<agent>/{shared,claude,codex}/prompt.md`; absence means unsupported unless an explicit loader rule says otherwise |
| Process role → agent selection | `config/processes.yaml` |
| Claude global tools/defaults | `config/harnesses/claude.yaml` |
| Codex lane defaults | `config/harnesses/codex.yaml` |
| Conversation context enum | Rust `ChatContextType` + TS `CONTEXT_TYPE_VALUES` |
| Store-key behavior | `frontend/src/lib/chat-context-registry.ts` |

Do not maintain a hand-written full agent roster here; canonical metadata and catalog tests own it. `config/ralphx.yaml` is compatibility/runtime configuration, not the live agent catalog.

## Conversation Contexts

| Context | Store-key family | Purpose |
|---|---|---|
| `ideation` | `session:` | Planning/proposal conversation |
| `delegation` | `delegation:` | Parent-linked delegated agent conversation |
| `task` | `task:` | Task Q&A |
| `project` | `project:` | Project Q&A |
| `standalone` | `standalone:` | Projectless Chat or Persona conversation in a private workspace |
| `task_execution` | `task_execution:` | Implementation run |
| `review` | `review:` | Review run/chat |
| `merge` | `merge:` | Merge resolution |
| `branch_update` | `branch_update:` | Branch synchronization/conflict resolution |

When adding a context, update Rust serialization/parsing, TS schema, registry/store key, send/resume routing, lifecycle events, queue/recovery behavior, and user-visible tests together.

## Agent Conversation Modes

| Mode | Contexts | Purpose |
|---|---|---|
| `persona_builder` | `project` \| `standalone` | Scope-locked persona build/refine in the Agents surface; folders are references and text attachments use the private workspace. |

## Verification Ownership

Current plan verification is backend-orchestrated: the model may choose allowed lenses/delegates, while backend state owns dispatch, settlement, lineage, and terminal classification. See `.claude/rules/ideation-verification-architecture.md`.

## MCP Frontmatter Scope

Canonical `capabilities.mcp_tools` owns the per-agent grant. Claude-specific generated frontmatter and Task/team constraints are materialized for that harness; Codex/native delegation use their own runtime paths. See `.claude/rules/agent-mcp-tools.md`.

## Lifecycle Invariant

Every active context must handle `agent:run_started`, message invalidation, turn completion, terminal completion/error/stop, queue delivery, and recovery without leaving a stale running state. See `.claude/rules/event-coverage-checklist.md`.
