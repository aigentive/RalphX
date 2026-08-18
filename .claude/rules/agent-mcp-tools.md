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
| Provider-native subagent | Use generated explicit tool entries when that harness surface requires them; do not generalize Claude frontmatter or `mcpServers` behavior to Codex. |
| Codex | Load canonical MCP capabilities through Codex runtime overrides/sidecars; do not reuse Claude plugin/frontmatter assumptions. |
| RalphX-native delegation | `delegate_start` caller→target authorization and delegation-tool visibility derive from `delegation.allowed_targets`; caller identity is transport-owned. |
| Delegate task coordination | Generic task tools stay scoped to the delegated session; `get/complete/release_delegate_assignment` use trusted current conversation/run headers to address only the exact caller task assignment. |
| Mixed external/internal transport | Public/high-level `mcp_tools` and `harnesses.<harness>.internal_mcp_tools` remain separate surfaces. |
| Provider-native third-party MCP | Native definitions/auth/trust remain provider-owned; canonical RalphX grants authorize only RalphX tools, while global/project policy may deny native servers/tools at launch. |

Only `tools` and `disallowedTools` are valid Claude agent frontmatter fields; `allowedTools` is a CLI flag, not a frontmatter key.

## Adding A New MCP Tool

- Backend: add the contained handler and route under `src-tauri/src/http_server/**`.
- MCP: add the schema to a focused `*-tools.ts` module and dispatch it in `index.ts`.
- Agent: grant it only to canonical agents/profiles whose prompt contract gives them a reason to use it.
- Validation: assert both allowed and denied agents, unknown-tool behavior, backend payload shape, and any side-effect guard.

## Runtime-Injected Role-Tiered Grants

Atlassian (Jira/Confluence) tools are granted per `RoutingRole` tier (`none|read|read_write`) and injected per spawn through the MCP runtime context, additively on top of canonical `capabilities.mcp_tools` — never through `agents/<agent>/agent.yaml` or generated frontmatter, which have no run/project/role context. Backend handlers re-derive the tier per request from the run's persisted `routing_role`/`project_id`. See `docs/features/atlassian-mcp-access.md`.

## Ticket Attachment Tools (NON-NEGOTIABLE)

`list_ticket_attachments` and `fetch_ticket_attachment` are read-only tools granted only to execution worker and coder surfaces. Access is tier-driven per request, not just spawn-time tool visibility: Jira calls re-derive the caller's persisted `routing_role` through the same `AtlassianMcpAccess` gate as the other Atlassian tools (`list_ticket_attachments`/`fetch_ticket_attachment` are part of `ATLASSIAN_READ_TOOLS`); Linear/ClickUp calls stay on the canonical worker/coder grant and a trusted, live caller-run identity check, never a fall-through with no check. `fetch_ticket_attachment` may return a materialized `contentPath` under RalphX-managed attachment storage (readable by Claude-harness runs; other sandboxed harnesses may not have filesystem access to it) plus an optional inline `contentText` preview for small `text/*` attachments. Raw provider URLs, credentials, and provider transport handles are still never exposed — see `plugins/app/ralphx-mcp-server/src/ticket-attachment-tools.ts` for the redaction allowlist/denylist that enforces this.

## Persona Builder Tools

`ralphx-persona-extractor` gets bounded `fs_*` reads plus `ask_user_question`, `save_persona_draft`, and `get_persona_draft`; context enters through the standard composer/read-root contract, not bespoke ingest commands or tools.

## Failure Diagnosis

| Symptom | Check |
|---|---|
| Tool absent | Canonical capability/profile, active harness transport, and generated runtime config |
| Tool listed but unavailable | Tool registry/schema and `index.ts` dispatch |
| Tool returns 404/schema error | Backend route and request type |
| Delegation tools disappear | Canonical `delegation.allowed_targets` and caller identity resolution |
| Agent is overgranted | Prompt contract vs canonical capabilities; remove stale grants and add a denied-path test |

Related rules: `agent-authoring.md` | `delegation-topology.md` | `multi-harness.md` | `mcp-servers.md`
