> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# Cross-Project Orchestration

## Overview

Cross-project orchestration lets an ideation session spawn implementation sessions in other RalphX projects on the same machine. This solves plans that span multiple codebases — e.g., a feature requiring changes in both a main app and a sibling MCP plugin.

The flow has three phases:

1. **Detection** — `cross_project_guide` scans a plan for file paths or keywords referencing other projects
2. **Gate** — the backend blocks proposal creation until detection has been run
3. **Execution** — agent calls `create_cross_project_session` and `migrate_proposals` to set up target sessions

---

## MCP Tools Reference

Four tools are available to the `ralphx-ideation` agent.

### `list_projects`

Discover all registered RalphX projects on this instance.

| Field | Value |
|-------|-------|
| Transport | `GET /api/internal/projects` |
| Parameters | none |

**Returns:** Array of `{ id, name, working_directory, task_count }` for each project.

**Use when:** Checking which projects are already registered before creating cross-project sessions (Step 2 of the 6-step workflow).

---

### `create_cross_project_session`

Export a verified plan to a target project by creating an inherited ideation session there.

| Field | Value |
|-------|-------|
| Transport | `POST /api/internal/cross_project/create_session` (HTTP) / `create_cross_project_session` (Tauri IPC) |
| Required params | `target_project_path`, `source_session_id` |
| Optional params | `title` |

| Parameter | Type | Description |
|-----------|------|-------------|
| `target_project_path` | string | Absolute filesystem path to the target project root. Backend resolves or auto-creates the RalphX project record. The directory must exist on disk. |
| `source_session_id` | string | ID of the source ideation session whose verified plan will be inherited. Source must be `Verified`, `Skipped`, or `ImportedVerified`. |
| `title` | string (optional) | Title for the new session. Defaults to `"Imported: {source session title}"`. |

**Returns:** `IdeationSessionResponse` — the newly created session with `inherited_plan_artifact_id` set and `verification_status = ImportedVerified`.

**Constraints:**
- Source plan must be verified (`Verified | Skipped | ImportedVerified`) — returns 400 otherwise
- Target path must exist on disk — returns 422 otherwise
- Circular imports are blocked (chain depth limit: 10) — returns 422 with `CIRCULAR_IMPORT` error code
- Auto-creates the RalphX project record if path not yet registered (emits `project:created` event)

---

### `cross_project_guide`

Analyze a plan for cross-project paths and set the backend gate to unlock proposal creation.

| Field | Value |
|-------|-------|
| Transport | Client-side regex (MCP server only — no HTTP round-trip for analysis); gate-setting via `POST /api/internal/sessions/:id/cross_project_check` |
| Required params | none (provide `session_id` OR `plan_content`) |

| Parameter | Type | Description |
|-----------|------|-------------|
| `session_id` | string (optional) | Session ID — tool fetches plan via `get_session_plan` internally. When provided: also sets the backend gate (`gate_status: "set"`). |
| `plan_content` | string (optional) | Raw plan text to analyze directly. When provided without `session_id`: analysis only, gate is not set (`gate_status: "no_session_id"`). |

**Returns:**

```json
{
  "has_cross_project_paths": true,
  "detected_paths": ["/Users/dev/reefagent-mcp-jira"],
  "guidance": "...",
  "gate_status": "set | no_session_id | backend_unavailable",
  "gate_error": "..."   // only present when gate_status = "backend_unavailable"
}
```

**Use when:** After creating or updating a plan — mandatory before creating proposals when `plan_artifact_id` is set.

---

### `migrate_proposals`

Copy proposals from a source session into a target session with dependency remapping.

| Field | Value |
|-------|-------|
| Transport | `POST /api/internal/cross_project/migrate_proposals` (HTTP) / `migrate_proposals` (Tauri IPC) |
| Required params | `source_session_id`, `target_session_id` |
| Optional params | `proposal_ids`, `target_project_filter` |

| Parameter | Type | Description |
|-----------|------|-------------|
| `source_session_id` | string | Source session to copy proposals from. |
| `target_session_id` | string | Target session to copy proposals into. |
| `proposal_ids` | string[] (optional) | Subset of proposal IDs to migrate. If omitted, all proposals are considered (subject to `target_project_filter`). |
| `target_project_filter` | string (optional) | Only migrate proposals whose `target_project` field matches this string. Useful for routing proposals to the correct target project. |

**Returns:**

```json
{
  "migrated": [
    { "source_id": "old-uuid", "target_id": "new-uuid" }
  ],
  "dropped_dependencies": [
    {
      "proposal_id": "...",
      "dropped_dep_id": "...",
      "reason": "Dependency target '...' was not included in the migration set"
    }
  ]
}
```

**Dependency remapping rules:**
- Both ends in migration set → remapped to new IDs ✅
- One end outside migration set → dropped with warning in `dropped_dependencies`
- Neither end in migration set → silently skipped

**Traceability:** Each migrated proposal gets `migrated_from_session_id` and `migrated_from_proposal_id` set automatically.

---

## Detection Flow

`cross_project_guide` runs entirely in the MCP server (TypeScript) — no backend round-trip for analysis.

### Path Regex Patterns

Three patterns are applied against the plan text:

| Pattern | Matches |
|---------|---------|
| `/(?:^|\s|["'\`])(\/(home\|Users\|workspace\|projects\|srv\|opt)\/[^\s"'\`]+)/gm` | Absolute paths under common Unix directories |
| `/(?:^|\s|["'\`])(\.\.\/?[^\s"'\`]+)/gm` | Relative `../` parent directory references |
| `/(?:target[_-]?project[_-]?path\|project[_-]?path\|working[_-]?directory)[:\s]+["']?([^\s"'\`,\n]+)/gim` | Key-value declarations (e.g., `target_project_path: /Users/dev/other`) |

### Keyword Matching

If path regex finds no matches, semantic keywords are checked (case-insensitive):

```
cross[-]?project | multi[-]?project | target project | another project |
different project | project b | separate repo | separate repository |
new repo | new repository | different codebase | other codebase |
monorepo boundary | external package | external module
```

### Decision Logic

```
has_cross_project_paths = (detected_paths.length > 0) OR (keyword_regex.test(plan_text))
```

### ASCII Flow

```
plan text (from session_id or plan_content)
         │
         ▼
  Apply 3 path regex patterns
         │
         ├─ paths found? → detected_paths = [...]
         │
         ▼
  Apply keyword regex (if no paths OR in addition)
         │
         ▼
  has_cross_project_paths = paths.length > 0 OR keyword match
         │
         ├─ true  → return { has_cross_project_paths: true, detected_paths: [...] }
         │          if session_id provided → POST /sessions/:id/cross_project_check
         │                                   → gate_status: "set"
         │
         └─ false → return { has_cross_project_paths: false }
                    gate still set (agent confirmed: no cross-project work)
```

---

## Backend Gate Mechanism

The gate prevents proposal creation on sessions with a plan that haven't been cross-project-checked.

### Column

`cross_project_checked BOOLEAN NOT NULL DEFAULT 1` on `ideation_sessions` — added by migration v72.

### Default Values by Session Type

| Session Type | `cross_project_checked` | Why |
|---|---|---|
| New session (just created) | `false` | Builder default — must call `cross_project_guide` before proposals |
| Imported via `create_cross_project_session` | `true` | Set explicitly in `create_cross_project_session_impl` |
| Child session (linked via `create_child_session`) | `true` | Set explicitly in `create_child_session_impl` |
| Existing rows (pre-migration v72) | `true` | Migration DEFAULT 1 — grandfathered in |

### Gate Enforcement

Location: `http_server/helpers.rs` → `create_proposal_impl()` — runs inside `db.run_transaction()`.

```
if session.plan_artifact_id.is_some() && !session.cross_project_checked {
    return Err("Cross-project check required: call cross_project_guide before creating proposals")
}
```

The gate only fires when the session has a `plan_artifact_id` (i.e., a real plan exists). Sessions without a plan are not gated.

### Gate-Setting Endpoint

`POST /api/internal/sessions/:id/cross_project_check` — sets `cross_project_checked = 1`.

Called automatically by `cross_project_guide` when `session_id` is provided. Returns `200 OK` on success, `404` if session not found.

### Gate Failure Recovery

When the gate blocks proposal creation:

1. Agent receives error: `"Cross-project check required: call cross_project_guide before creating proposals"`
2. Recovery: call `cross_project_guide` with `session_id` to run analysis and set the gate
3. Gate is set regardless of `has_cross_project_paths` result — calling the tool is sufficient

---

## Agent Auto-Prompt Guidance

When `cross_project_guide` returns `has_cross_project_paths: true`, agents follow this mandatory 6-step workflow defined in `agents/ralphx-ideation/claude/prompt.md`.

### 6-Step Workflow

1. **Present detected paths** — show the user the detected project paths from the `cross_project_guide` response
2. **Check `list_projects`** — call `list_projects` and match each detected path against `working_directory` fields to see which projects are already registered
3. **Inform about auto-registration** — for any detected path not found in `list_projects`, tell the user: _"This project isn't registered yet — `create_cross_project_session` will auto-register it from the directory"_
4. **Confirm with user** — call `ask_user_question` with: _"Create implementation sessions in these projects? [Y/n]"_ listing each target project path
5. **On confirmation** — call `create_cross_project_session` for each confirmed target project directory
6. **Tag proposals with `target_project`** — when creating proposals in Phase 5 PROPOSE, set the `target_project` field to route each proposal to the correct project session

**If `has_cross_project_paths: false`** — proceed normally, no user prompt needed.

### Concrete Example

```
Plan contains: "/Users/dev/reefagent-mcp-jira/src/jira-tool.ts"

→ cross_project_guide returns:
    has_cross_project_paths: true
    detected_paths: ["/Users/dev/reefagent-mcp-jira"]
    gate_status: "set"

→ list_projects → "/Users/dev/reefagent-mcp-jira" not found in results

→ ask_user_question:
    "I detected implementation work in another project:
     - /Users/dev/reefagent-mcp-jira (not yet registered in RalphX)

     Create implementation sessions in these projects? [Y/n]"

→ User confirms
  → create_cross_project_session(
        target_project_path: "/Users/dev/reefagent-mcp-jira",
        source_session_id: "current-session-id"
    )
  → Returns new session with id = "new-session-xyz"

→ In Phase 5 PROPOSE:
    create_task_proposal(..., target_project: "/Users/dev/reefagent-mcp-jira")
    for all proposals belonging to that project

→ After proposals created:
    migrate_proposals(
        source_session_id: "current-session-id",
        target_session_id: "new-session-xyz",
        target_project_filter: "/Users/dev/reefagent-mcp-jira"
    )
```

---

## HTTP Routes

All routes served by Axum on `:3847` (internal only — not accessible from external MCP).

| Method | Route | Handler | Purpose |
|--------|-------|---------|---------|
| `GET` | `/api/internal/projects` | `list_projects_internal` | List all projects |
| `POST` | `/api/internal/cross_project/create_session` | `create_cross_project_session_http` | Export plan to target project |
| `POST` | `/api/internal/cross_project/migrate_proposals` | `migrate_proposals_http` | Copy proposals between sessions |
| `POST` | `/api/internal/sessions/:id/cross_project_check` | `set_cross_project_checked` | Set gate after `cross_project_guide` analysis |

---

## Tauri Commands

Two Tauri IPC commands mirror the HTTP endpoints (same implementation, shared `_impl` functions).

| Command | Parameters | Caller |
|---------|-----------|--------|
| `create_cross_project_session` | `input: { targetProjectPath, sourceSessionId, title? }` | `useExportPlanToProject` hook |
| `migrate_proposals` | `input: { sourceSessionId, targetSessionId, proposalIds?, targetProjectFilter? }` | (direct agent use via MCP; no dedicated frontend hook) |

**Note:** Both Tauri commands use `#[serde(rename_all = "camelCase")]` — pass camelCase fields in `invoke()`.

---

## Key Files Reference

| File | Purpose |
|------|---------|
| `plugins/app/ralphx-mcp-server/src/index.ts` | `cross_project_guide` implementation: regex patterns, `CROSS_PROJECT_KEYWORDS`, gate-setting call |
| `plugins/app/ralphx-mcp-server/src/tools.ts` | MCP tool definitions for `list_projects`, `create_cross_project_session`, `cross_project_guide`, `migrate_proposals` |
| `plugins/app/ralphx-mcp-server/src/__tests__/cross-project-guide.test.ts` | Unit tests for keyword detection |
| `agents/ralphx-ideation/claude/prompt.md` | 6-step workflow prompt (Phase 4 cross-project section) |
| `src-tauri/src/commands/ideation_commands/ideation_commands_cross_project.rs` | `create_cross_project_session` and `migrate_proposals` Tauri command implementations |
| `src-tauri/src/commands/ideation_commands/ideation_commands_types.rs` | `CreateCrossProjectSessionInput`, `MigrateProposalsInput`, `MigrateProposalsResult` types |
| `src-tauri/src/http_server/handlers/internal.rs` | `create_cross_project_session_http`, `migrate_proposals_http`, `set_cross_project_checked` handlers |
| `src-tauri/src/http_server/mod.rs` | Route registration for all 4 cross-project HTTP endpoints |
| `src-tauri/src/http_server/helpers.rs` | `create_proposal_impl` — gate enforcement logic |
| `src-tauri/src/infrastructure/sqlite/migrations/v72_cross_project_check.rs` | Adds `cross_project_checked` column (DEFAULT 1) |
| `src-tauri/src/infrastructure/sqlite/migrations/v66_cross_project_import.rs` | Adds `source_project_id`, `source_session_id` columns for import traceability |
| `src-tauri/src/domain/entities/ideation/mod.rs` | `IdeationSession` entity — `cross_project_checked` field default (`false`) |
| `src/hooks/useExportPlanToProject.ts` | Frontend hook wrapping `create_cross_project_session` Tauri command |
