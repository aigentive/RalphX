---
paths:
  - "agents/**"
  - "config/harnesses/**"
  - "config/processes.yaml"
  - "config/ralphx.yaml"
  - "plugins/app/ralphx-mcp-server/src/**"
  - "src-tauri/src/infrastructure/agents/**"
  - "src-tauri/src/http_server/**"
---

> **Maintainer note:** Keep this file compact. Prefer one-line rules, links to source docs, and explicit non-negotiables over prose.

# Agent MCP Tool Alignment

## Canonical Ownership

| Concern | Source of truth |
|---|---|
| Per-agent MCP grant | `agents/<agent>/agent.yaml` `capabilities.mcp_tools` |
| Profile-specific MCP grant | `agents/<agent>/agent.yaml` `profiles.<profile>.capabilities.mcp_tools` |
| RalphX-native delegation rights | `agents/<agent>/agent.yaml` `delegation.allowed_targets` |
| Claude native tools/model/effort | `agents/<agent>/agent.yaml` `harnesses.claude` + named sets in `config/harnesses/claude.yaml` |
| Codex runtime features | `agents/<agent>/agent.yaml` `harnesses.codex`; lane defaults in `config/harnesses/codex.yaml` |
| Team-process ceilings | `config/processes.yaml` |
| MCP tool schema | focused `plugins/app/ralphx-mcp-server/src/*-tools.ts` module |
| MCP dispatch | `plugins/app/ralphx-mcp-server/src/index.ts` |
| MCP authorization | `tool-authorization.ts` loading canonical metadata; `tools.ts` is a registry/facade |
| Legacy compatibility | Only an explicitly documented row in `config/ralphx.yaml` or `LEGACY_TOOL_ALLOWLIST`; never add new canonical ownership there |

## Alignment Rule (NON-NEGOTIABLE)

When a tool is added, removed, or renamed for an agent:

1. Update the live prompt contract only when the agent needs workflow instructions for that tool.
2. Update canonical `capabilities.mcp_tools` for the agent/profile.
3. Add/remove the tool schema and `index.ts` dispatch when the tool itself changes.
4. Keep backend route/request types aligned.
5. Rebuild `plugins/app/ralphx-mcp-server` after any `src/` change.
6. Run canonical catalog, authorization, and focused tool-schema tests.

Prompts are contracts, not migration diaries: remove dead tool prose; keep compatibility enforcement in metadata/runtime/tests.

## Effective Authorization

`tool-authorization.ts` resolves grants in this order, then applies canonical delegation policy:

1. `RALPHX_ALLOWED_MCP_TOOLS` — standalone test/debug override.
2. `--allowed-tools` — runtime-injected grant list.
3. Canonical `agents/<agent>/agent.yaml` capabilities, including the active profile.
4. Explicit legacy allowlist — compatibility only; currently empty for live canonical agents.
5. Empty list — fail closed.

Do not edit a `TOOL_ALLOWLIST` mirror to grant production access. The compatibility mirror is generated from canonical metadata.

## Harness-Specific Rules

| Path | Rule |
|---|---|
| Backend-spawned Claude | Rust materializes canonical metadata and injects the effective CLI/MCP configuration. |
| Claude Task/team subagent | Use generated explicit tool entries when that Claude surface requires them; do not generalize its frontmatter or `mcpServers` behavior to Codex. |
| Codex | Load canonical MCP capabilities through Codex runtime overrides/sidecars; do not reuse Claude plugin/frontmatter assumptions. |
| RalphX-native delegation | `delegate_start` caller→target authorization and delegation-tool visibility derive from `delegation.allowed_targets`; caller identity is transport-owned. |
| Mixed external/internal transport | Public/high-level `mcp_tools` and `harnesses.<harness>.internal_mcp_tools` remain separate surfaces. |

Only `tools` and `disallowedTools` are valid Claude agent frontmatter fields; `allowedTools` is a CLI flag, not a frontmatter key.

## Adding A New MCP Tool

- Backend: add the contained handler and route under `src-tauri/src/http_server/**`.
- MCP: add the schema to a focused `*-tools.ts` module and dispatch it in `index.ts`.
- Agent: grant it only to canonical agents/profiles whose prompt contract gives them a reason to use it.
- Validation: assert both allowed and denied agents, unknown-tool behavior, backend payload shape, and any side-effect guard.

## Failure Diagnosis

| Symptom | Check |
|---|---|
| Tool absent | Canonical capability/profile, active harness transport, and generated runtime config |
| Tool listed but unavailable | Tool registry/schema and `index.ts` dispatch |
| Tool returns 404/schema error | Backend route and request type |
| Delegation tools disappear | Canonical `delegation.allowed_targets` and caller identity resolution |
| Agent is overgranted | Prompt contract vs canonical capabilities; remove stale grants and add a denied-path test |

Related rules: `agent-authoring.md` | `delegation-topology.md` | `multi-harness.md` | `mcp-servers.md`
