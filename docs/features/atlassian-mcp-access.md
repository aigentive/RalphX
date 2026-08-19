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
| `read` | `jira_search_issues`, `jira_get_issue`, `jira_list_projects`, `jira_list_transitions`, `jira_list_boards`, `jira_list_sprints`, `jira_get_sprint_issues`, `jira_list_comments`, `jira_search_users`, `confluence_search_pages`, `confluence_get_page`, `confluence_list_spaces`, `atlassian_api_request` (GET/HEAD only), `list_ticket_attachments`, `fetch_ticket_attachment` (Jira calls only — see below) |
| `read_write` | Everything in `read`, plus `jira_create_issue`, `jira_update_issue`, `jira_add_comment`, `jira_transition_issue`, `jira_assign_issue`, `confluence_create_page`, `confluence_update_page`, and mutating `atlassian_api_request` methods |

`list_ticket_attachments` and `fetch_ticket_attachment` are also granted to
worker/coder through their canonical `agent.yaml` `capabilities.mcp_tools`,
independent of this tier system, because they cover Linear and ClickUp
attachments too. Only the *Jira* provider call re-derives and enforces the
tier above (`authorize_ticket_attachment_access` in
`src-tauri/src/http_server/handlers/ticket_attachments.rs`); Linear/ClickUp
calls stay on the canonical grant plus a trusted, live caller-run identity
check — never a fall-through with no check at all. `fetch_ticket_attachment`
may return a materialized `contentPath` under RalphX-managed attachment
storage, readable by Claude-harness runs, plus an optional inline
`contentText` preview for small `text/*` attachments; other sandboxed
harnesses may not have filesystem access to that path.

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

## Jira Software (Agile) Tools

`jira_list_boards`, `jira_list_sprints`, and `jira_get_sprint_issues` cover the
board/sprint surface at the `read` tier:

- `jira_list_boards` accepts an optional `projectKey` filter; omitting it lists
  every board visible to the credential.
- `jira_list_sprints` currently returns only active sprints. An explicit
  `state` other than `"active"` is rejected rather than silently ignored.
- `jira_get_sprint_issues` returns up to 50 issues per sprint, using the same
  enriched summary shape (status, issue type, assignee, updated timestamp) as
  `jira_search_issues`.

## Discovery Tools

`jira_list_comments`, `jira_search_users`, and `confluence_list_spaces` cover
gaps that otherwise leave agents guessing at ids or unable to see comments
beyond the handful inlined by `jira_get_issue`, all at the `read` tier:

- `jira_list_comments` pages through an issue's comments (`startAt`/`maxResults`,
  default 20, capped at 100) and returns the provider's true `total` alongside
  markdown-converted bodies (the same ADF→markdown conversion `jira_get_issue`
  uses).
- `jira_search_users` is a bounded (max 20) name/address search returning
  `accountId`/`displayName`, used to resolve an `accountId` for
  `jira_assign_issue` or `jira_create_issue`'s `assigneeAccountId`.
- `confluence_list_spaces` lists Confluence spaces (`id`/`key`/`name`) via the
  v2 API, unblocking `confluence_create_page`'s otherwise-unguessable
  `spaceId`.

`jira_assign_issue` accepts an optional `accountId` in addition to
`assignToMe`. Precedence: `accountId` (if present) wins, then `assignToMe`,
then the issue's assignee is cleared.

`jira_create_issue` additionally accepts `parentKey` (epic/parent link, maps to
`fields.parent`), `assigneeAccountId`, and `components`.

When an issue has more comments than the inline prompt-expansion budget shows,
the rendered reference body now reports the provider's true comment total and
points agents at `jira_list_comments` instead of a bare omitted-count.

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
| Ticket attachment tools (Jira/Linear/ClickUp) | `src-tauri/src/http_server/handlers/ticket_attachments.rs`, `plugins/app/ralphx-mcp-server/src/ticket-attachment-tools.ts` |
| Roles editor control | `frontend/src/components/settings/AgentRoleDefaultEditor.tsx` |
