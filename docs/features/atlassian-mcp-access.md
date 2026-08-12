# Atlassian MCP Access

Built-in Jira + Confluence MCP tools for RalphX agents, proxied through the
backend so they reuse the already-configured Atlassian integration credentials
(both API-token and OAuth modes, including server-side token refresh).

This does not replace the existing Atlassian integration — it adds direct agent
tool calls on top of it.

## Tiers

| Tier | Grants |
|---|---|
| `none` | No Atlassian tools at all |
| `read` | `jira_search_issues`, `jira_get_issue`, `jira_list_projects`, `jira_list_transitions`, `confluence_search_pages`, `confluence_get_page`, `atlassian_api_request` (GET/HEAD only) |
| `read_write` | Everything in `read`, plus `jira_create_issue`, `jira_update_issue`, `jira_add_comment`, `jira_transition_issue`, `jira_assign_issue`, `confluence_create_page`, `confluence_update_page`, and mutating `atlassian_api_request` methods |

## Built-in Defaults

| Routing role | Default tier |
|---|---|
| `workspace_edit`, `workspace_pr_fixer`, `execution_worker`, `execution_reexecutor` | `read_write` |
| Every other routing role | `read` |

Defaults live in `default_atlassian_access` (`src-tauri/crates/ralphx-domain/src/agents/atlassian_mcp_access.rs`).
The match is exhaustive on purpose: adding a routing role forces an explicit
read/write decision rather than inheriting a catch-all.

## Enablement

Tools follow the main Atlassian integration switch. The integration must be
**enabled and validated** (`validation_status == valid`) — `enabled` alone is
not sufficient, matching the predicate `enabled_auth_context_for_settings`
already enforces. Existing installs get the tools automatically after update
because enablement is derived live, never backfilled into settings.

## Overrides

Settings > Agents > Roles > Edit > Permissions has an **Atlassian** select with
`Role default` / `None` / `Read` / `Read + write`. It is disabled with a hint
when the integration is not usable.

The override is stored as an optional `atlassianAccess` field on the role's
`manual_role_defaults` row and resolves through the existing 6-layer precedence
(project UI row → project `.ralphx/router.yaml` → global UI row → global router
YAML → legacy lane settings → provider default).

**Resolution is row-wins, not per-field merge.** The first matching layer
supplies the whole row, so a project row that omits `atlassianAccess` falls back
to the *built-in role default*, not to the global row's value. This matches
every other field on that struct.

## Enforcement

Two independent layers:

1. **Spawn-time visibility** — the tier is resolved at spawn and injected into
   the harness tool allowlist (Claude `--allowed-tools`, Codex `enabled_tools`).
   Agents never see tools above their tier. The grant is *additive*: it extends
   the agent's canonical `agent.yaml` allowlist rather than replacing it.
2. **Per-request enforcement** — every backend endpoint re-derives the tier from
   the run's persisted routing role and project id. Lowering a role or disabling
   the integration therefore takes effect immediately for in-flight sessions,
   and at next spawn for visibility.

The tier itself is never persisted. Only the authoritative `routing_role` and
`project_id` are stored on `agent_runs`, so enforcement always reads current
configuration.

Everything fails closed: a missing routing role, an unresolvable project, a
repository error, or an unusable integration all resolve to `none`.

`AgentRun.launch_role` is display attribution covering three agents and is never
consulted for authorization.

## Escape Hatch

`atlassian_api_request` covers the API long tail. Containment rules:

- relative paths only; absolute URLs, protocol-relative paths, backslashes, and
  control characters are **rejected, never sanitized**
- the path must start with `/rest/api/`, `/rest/agile/`, `/wiki/rest/api/`, or
  `/wiki/api/v2/`
- no `..` segments
- responses are size-bounded
- the HTTP method decides the required tier: GET/HEAD need `read`, everything
  else needs `read_write`

Validation runs at the handler *and* again at the request sink in the client.

## Known Limitations

- **Claude-native `Task` subagents are out of scope.** They inherit generated
  plugin frontmatter, which is materialized without run, project, or role
  context and is shared across all spawns of an agent. Role-tiered tools reach
  RalphX-spawned agents and RalphX-native delegates only.
- **Runs started before this feature** have no persisted routing role and are
  denied until respawned.
- **No local rate limiter.** Atlassian 429s surface as structured tool errors
  carrying the numeric status.
- **Jira rich text is plain text only.** Descriptions and comments are wrapped
  into a minimal ADF paragraph for Jira Cloud v3.

## Key Files

| Concern | Path |
|---|---|
| Tier model + built-in defaults | `src-tauri/crates/ralphx-domain/src/agents/atlassian_mcp_access.rs` |
| Effective-access resolution | `src-tauri/src/application/atlassian_mcp_access.rs` |
| Service operations | `src-tauri/src/application/atlassian_mcp_service.rs` |
| Client operations + containment | `src-tauri/src/infrastructure/atlassian_mcp_client.rs` |
| HTTP endpoints + authorization | `src-tauri/src/http_server/handlers/atlassian_mcp/` |
| MCP tool schemas + dispatch | `plugins/app/ralphx-mcp-server/src/atlassian-tools.ts` |
| Roles editor control | `frontend/src/components/settings/AgentRoleDefaultEditor.tsx` |
