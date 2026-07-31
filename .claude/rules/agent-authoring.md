---
paths:
  - "agents/**"
  - "config/**"
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
| Claude-specific metadata | `agents/<agent>/agent.yaml` `harnesses.claude`; `claude/agent.yaml` is legacy fallback only |
| Codex-specific prompt | `agents/<agent>/codex/prompt.md` |
| Per-agent MCP grants | `agents/<agent>/agent.yaml` `capabilities.mcp_tools` |
| Claude-global tool sets / defaults | `config/harnesses/claude.yaml` |
| Codex lane defaults | `config/harnesses/codex.yaml` |
| Process mapping / team constraints | `config/processes.yaml` |
| MCP registration / dispatch | `plugins/app/ralphx-mcp-server/src/*-tools.ts` and `index.ts` |
| Agent short-name constants | `plugins/app/ralphx-mcp-server/src/agentNames.ts` and `src-tauri/src/infrastructure/agents/claude/agent_names.rs` |
| Canonical profile-key validation | `trusted_canonical_profile_name` (`src-tauri/src/infrastructure/agents/harness_agent_catalog.rs`) and `SAFE_CANONICAL_PROFILE_NAME` (`plugins/app/ralphx-mcp-server/src/canonical-agent-metadata.ts`) must identically enforce `^[a-z0-9]+(?:[_-][a-z0-9]+)*$`; change both together |

**Rule:** Do not create or edit authored prompt files under `plugins/app/agents/`. Claude plugin markdown is generated from the canonical `agents/` tree.

## Add A New Agent

| Step | Required action |
|---|---|
| 1 | Pick the canonical agent id and add `agents/<agent>/agent.yaml` |
| 2 | Add prompt files: `shared/prompt.md` only if truly harness-neutral, otherwise add `<harness>/prompt.md` per supported harness |
| 3 | Put Claude-only model/tools/effort/permissions/skills under root `agent.yaml` `harnesses.claude` |
| 4 | Declare model/tools/runtime features under `harnesses.<harness>` and MCP grants under `capabilities.mcp_tools` in canonical metadata |
| 5 | If the agent needs MCP tools, align canonical capability metadata, the live prompt contract, and MCP registration/dispatch |
| 6 | Add/update agent name constants if the agent is referenced by MCP or Rust agent-name maps |
| 7 | Add or extend tests proving canonical loadability and harness-specific behavior |

## Modify An Existing Agent

| Change type | Where to edit |
|---|---|
| Role / description / shared identity | `agents/<agent>/agent.yaml` |
| Claude-only prompt behavior | `agents/<agent>/claude/prompt.md` |
| Codex-only prompt behavior | `agents/<agent>/codex/prompt.md` |
| Shared prompt wording | `agents/<agent>/shared/prompt.md` |
| Claude harness/frontmatter behavior | `agents/<agent>/agent.yaml` `harnesses.claude` |
| Per-agent model / tools / MCP grants | `agents/<agent>/agent.yaml` |
| Harness-global defaults | `config/harnesses/<harness>.yaml` |

## Prompt Split Rules

| Rule | Detail |
|---|---|
| Prefer shared prompts only when semantics are actually neutral | If Codex or Claude needs harness-specific delegation/tooling language, split the prompt |
| Unsupported harnesses stay explicit | No prompt file for that harness means unsupported; do not silently inherit another harness prompt |
| Canonical Claude metadata lives in root `agent.yaml` | Prefer `harnesses.claude.*` in root `agents/<agent>/agent.yaml`; `claude/agent.yaml` is legacy fallback only |
| Prompts are contracts, not migration diaries | Keep prompts limited to the live role, live tool surface, and output contract; put migration notes, removed-tool warnings, and compatibility ballast in tests/docs/runtime enforcement instead |
| Profile prompts replace base prompts | `agents/<agent>/profiles/<profile>/<harness>/prompt.md` fully replaces the base prompt; there is no `shared/` fallback, so the profile prompt must be self-contained |

## MCP / Tool Checklist

When adding or removing MCP tools from an agent:
1. Update canonical prompt instructions if the tool contract changed
2. Update `agents/<agent>/agent.yaml` `capabilities.mcp_tools`
3. Register or remove the handler in the focused MCP tool module and `index.ts`
4. Validate prompt tool examples against MCP schemas and backend request types
5. Rebuild the MCP server if TypeScript changed

See `agent-mcp-tools.md` for the strict alignment rule.

## Required Tests

| Test type | What it proves |
|---|---|
| Canonical catalog test | `agents/<agent>/agent.yaml` and prompt files load cleanly |
| Claude generation test | generated Claude artifact matches canonical body/metadata and runtime tool config |
| Codex hygiene test | Codex prompt contains no Claude-only syntax when the agent is cross-harness |
| Runtime config test | canonical metadata resolves into the effective harness/runtime configuration |
| Prompt schema contract test | prompt tool examples and required payloads satisfy live MCP schemas and backend request types |
| Profile catalog load-contract test | adding a profile requires proof that the catalog can load it; selecting its string alone is not coverage |

## Fast Failure Rules

| Don’t do this | Why |
|---|---|
| Reintroduce authored `plugins/app/agents/*.md` files | That revives the old split-brain source-of-truth problem |
| Add new ownership to `claude/agent.yaml` or other legacy fallback files | New harness-specific ownership belongs under root `agent.yaml` `harnesses.<harness>` |
| Reuse a Claude prompt for Codex by omission | Unsupported harnesses must fail clearly, not inherit accidentally |
| Change MCP tools in only one layer | The agent will drift between prompt contract, runtime config, and server allowlist |
| Change profile-key validation in one layer | Rust and MCP validation must stay identical; update `trusted_canonical_profile_name` and `SAFE_CANONICAL_PROFILE_NAME` together |
