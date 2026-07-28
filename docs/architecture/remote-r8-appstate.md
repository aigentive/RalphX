# R-8 — AppState identity behind the remote fetch remount

The remote listener (`:3849`) serves a curated subset of the UI's own `/api` routes so a paired
device can read the same data the local app reads. R-8 is the guarantee that it reads that data
from the **same memory**, not from a look-alike copy.

## The problem R-8 closes

RalphX already runs two `AppState` graphs:

| Graph | Built by | Consumed by |
|---|---|---|
| Tauri-managed | app setup | every `#[tauri::command]`, and therefore the remote **invoke** facade |
| `:3847` HTTP | `build_http_app_state` | every `/api` handler |

`build_http_app_state` deliberately Arc-shares the authority-bearing fields between them. If the
fetch remount had constructed a **third** graph, a remote client could see one answer through
`/invoke` and a different answer through a proxied fetch — the same class of divergence the
big-PR checklist calls DRIFT. Slice B therefore reuses the `:3847` Arc rather than rebuilding.

## Mechanism

`SharedHttpAppState` (`src-tauri/src/remote_server/fetch_remount.rs`) is a newtype over the
`Arc<AppState>` and `Arc<ExecutionState>` that `:3847` is about to serve from. It is registered
as Tauri-managed state at the `server_boot` seam — the one place holding both Arcs and the
`AppHandle` — and read back when the remote listener builds its router.

```
server_boot ──manage──▶ Arc<SharedHttpAppState> ──try_state──▶ RemoteRouterState.remount
                                                                      │
                                                                      ▼
                                                        remount_router → HttpServerState
```

**Fail-closed.** If the newtype is not managed, `remount_router` is never called: the `/api`
routes are not mounted at all and answer with the listener's normal
`REMOTE_COMMAND_UNAVAILABLE` 404. There is no fallback that builds a fresh `AppState`.
Pinned by `without_shared_state_no_api_route_is_mounted`.

## Shared vs fresh

Two questions matter, and they are different:

1. **Does the remount share with `:3847`?** Yes, totally — it is the same `Arc`, so every field
   is shared by construction. `the_shared_state_hands_out_the_same_arcs_it_was_built_from` and
   `every_resolution_of_the_shared_state_yields_the_same_arcs` assert `Arc::ptr_eq`.
2. **Does `:3847` share with the Tauri-managed graph?** Field by field, per the table below.

### Fields `build_http_app_state` Arc-shares with the managed `AppState`

`db` (inner connection) · `question_state` · `permission_state` · `message_queue` ·
`queued_message_repo` · `interactive_process_registry` · `github_service` ·
`pr_poller_registry` · `events` · `internal_event_bus` · `app_paths` · `window_focus_state` ·
`notification_service_cache` · `agent_capability_gate` · `streaming_state_cache` ·
`webhook_publisher` · `session_merge_locks` · `startup_coordinator` (via
`share_startup_coordinator`) · `plan_verification_locks` and `plan_verification_admissions`
(via `share_plan_verification_runtime`).

`the_r8_shared_field_enumeration_matches_build_http_app_state` pins this list against the live
source, so a field quietly dropped from the sharing list fails a test instead of surfacing as a
remote client reading different authority state than the local UI.

### Fields that are fresh — and why no mounted route cares

Every **repository** not named above is a fresh struct in the `:3847` clone, but each one is
constructed over the **shared `db` connection**. Their reads are therefore identical to the
managed graph's; freshness of the struct is not freshness of the data. All eight mounted routes
read exclusively through repositories (`ideation_session_repo`, `artifact_repo`,
`agent_task_repo`, `agent_workflow_repo`, `agent_run_repo`) plus `message_queue`, which is
shared outright.

`DelegationService` is fresh and stays fresh. It backs `/api/internal/*` only, which this slice
never mounts. `no_mounted_route_touches_delegation_service` scans the eight mounted handler
**bodies** (not their whole files — `agent_workflows.rs` also holds the workflow runner, which
legitimately uses the delegation service) and fails if any of them reads it.

### Routes dropped because they read genuinely fresh in-memory state

| Route | Fresh field | Resolution |
|---|---|---|
| `GET /api/conversations/:id/active-state` | `running_agent_registry` | **Dropped.** Its `isActive` would diverge from the facade's view. Promoting the field would change `:3847` behavior, which is outside slice B's scope. |
| `GET /api/ideation/sessions/:id/child-status` | `running_agent_registry` | **Dropped**, same reason. |

Recording them here rather than silently omitting them is the point: v1 may be a subset, but the
subset must be explained.

## Related

- Allowlist, denied sinks and the scope gate: `src-tauri/src/remote_server/fetch_remount.rs`
- Tests: `src-tauri/src/remote_server/fetch_remount_tests.rs`
- Workflow separation this must not be confused with: `.claude/rules/agent-workspace-review-modes.md`
