---
paths:
  - "agents/**"
  - "config/ralphx.yaml"
  - "plugins/app/ralphx-mcp-server/src/**"
  - "src-tauri/src/infrastructure/agents/**"
  - "src-tauri/src/application/chat_service/**"
  - "src-tauri/src/commands/**"
  - "frontend/src/**"
  - "docs/architecture/harness-specific-agent-config.md"
---

# Agent Authoring

**Required Context:** `agent-mcp-tools.md` | `multi-harness.md` | `docs/architecture/harness-specific-agent-config.md`

## Canonical Source Of Truth

| Concern | Canonical location |
|---|---|
| Agent identity / shared metadata | `agents/<agent>/agent.yaml` |
| Harness-neutral prompt | `agents/<agent>/shared/prompt.md` |
| Claude-specific prompt | `agents/<agent>/claude/prompt.md` |
| Claude-specific metadata | `agents/<agent>/claude/agent.yaml` |
| Codex-specific prompt | `agents/<agent>/codex/prompt.md` |
| Runtime lane/tool config | `config/ralphx.yaml` |
| MCP allowlist / tool dispatch | `plugins/app/ralphx-mcp-server/src/tools.ts` |
| Agent short-name constants | `plugins/app/ralphx-mcp-server/src/agentNames.ts` and `src-tauri/src/infrastructure/agents/claude/agent_names.rs` |

**Rule:** Do not create or edit authored prompt files under `plugins/app/agents/`. Claude plugin markdown is generated from the canonical `agents/` tree.

## Add A New Agent

| Step | Required action |
|---|---|
| 1 | Pick the canonical agent id and add `agents/<agent>/agent.yaml` |
| 2 | Add prompt files: `shared/prompt.md` only if truly harness-neutral, otherwise add `<harness>/prompt.md` per supported harness |
| 3 | Add `claude/agent.yaml` only for real Claude-only metadata such as `disallowed_tools`, `skills`, `max_turns`, or frontmatter compatibility aliases |
| 4 | Register the runtime entry in `config/ralphx.yaml` with the canonical `system_prompt_file` path under `agents/` |
| 5 | If the agent needs MCP tools, update all three layers: canonical prompt contract, `config/ralphx.yaml` `mcp_tools`, and `plugins/app/ralphx-mcp-server/src/tools.ts` |
| 6 | Add/update agent name constants if the agent is referenced by MCP or Rust agent-name maps |
| 7 | Add or extend tests proving canonical loadability and harness-specific behavior |

## Modify An Existing Agent

| Change type | Where to edit |
|---|---|
| Role / description / shared identity | `agents/<agent>/agent.yaml` |
| Claude-only prompt behavior | `agents/<agent>/claude/prompt.md` |
| Codex-only prompt behavior | `agents/<agent>/codex/prompt.md` |
| Shared prompt wording | `agents/<agent>/shared/prompt.md` |
| Claude frontmatter behavior | `agents/<agent>/claude/agent.yaml` |
| Model / tools / MCP grants / runtime lane settings | `config/ralphx.yaml` |

## Prompt Split Rules

| Rule | Detail |
|---|---|
| Prefer shared prompts only when semantics are actually neutral | If Codex or Claude needs harness-specific delegation/tooling language, split the prompt |
| Unsupported harnesses stay explicit | No prompt file for that harness means unsupported; do not silently inherit another harness prompt |
| Canonical Claude metadata lives in root `agent.yaml` | Prefer `harnesses.claude.*` in root `agents/<agent>/agent.yaml`; `claude/agent.yaml` is legacy fallback only |
| Prompts are contracts, not migration diaries | Keep prompts limited to the live role, live tool surface, and output contract; put migration notes, removed-tool warnings, and compatibility ballast in tests/docs/runtime enforcement instead |

## MCP / Tool Checklist

When adding or removing MCP tools from an agent:
1. Update canonical prompt instructions if the tool contract changed
2. Update `config/ralphx.yaml` `mcp_tools`
3. Update `plugins/app/ralphx-mcp-server/src/tools.ts`
4. Validate prompt tool examples against MCP schemas and backend request types
5. Rebuild the MCP server if TypeScript changed

See `agent-mcp-tools.md` for the strict three-layer rule.

## Required Tests

| Test type | What it proves |
|---|---|
| Canonical catalog test | `agents/<agent>/agent.yaml` and prompt files load cleanly |
| Claude generation test | generated Claude artifact matches canonical body/metadata and runtime tool config |
| Codex hygiene test | Codex prompt contains no Claude-only syntax when the agent is cross-harness |
| Runtime config test | live `config/ralphx.yaml` entries point at canonical `agents/` prompt paths |
| Prompt schema contract test | prompt tool examples and required payloads satisfy live MCP schemas and backend request types |

## Fast Failure Rules

| Don’t do this | Why |
|---|---|
| Reintroduce authored `plugins/app/agents/*.md` files | That revives the old split-brain source-of-truth problem |
| Put Claude-only fields back into root `agent.yaml` | That muddies canonical vs harness-local ownership |
| Reuse a Claude prompt for Codex by omission | Unsupported harnesses must fail clearly, not inherit accidentally |
| Change MCP tools in only one layer | The agent will drift between prompt contract, runtime config, and server allowlist |
