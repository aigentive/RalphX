/**
 * Regression guard for the five orphan `invoke()` names (PR 3.1-a census batch `X1`).
 *
 * Each of these command names was invoked by a frontend wrapper but had NO
 * `#[tauri::command]` behind it in `generate_handler!`, so every call rejected at runtime.
 * None of the five was reachable from real UI — the wrappers were dead code, kept alive
 * only by their own unit tests — so all five were resolved by deleting the call site
 * rather than by adding host authority the product does not use:
 *
 * | Command | Why it went, not grew |
 * |---|---|
 * | `add_proposal_dependency` | Its owning hook `useDependencyMutations` has no consumer; the UI READS the dependency graph and never writes edges. Adding the command would have minted a mutation surface nothing exposes. |
 * | `create_child_session` | Zero callers, zero tests. The capability is real and still reachable — it is an HTTP route (`POST /api/create_child_session`), which is how agents use it. |
 * | `get_parent_session_context` | Zero callers, zero tests. Same HTTP route story (`GET /api/parent_session_context/:session_id`). |
 * | `delete_project` | Zero callers. `projectsApi.archive` is the live removal path. |
 * | `delete_task` | Explicitly `@deprecated Use cleanupTask instead`; every component destructures `cleanupTaskMutation`, never `deleteMutation`. |
 *
 * The remote transport drift scan already ratchets on reintroduction (a new invoke name
 * with no handler pushes the unclassified count above the baseline and fails CI). This
 * file is the fast local half of that guard: it fails in vitest, at the surface where the
 * mistake would actually be made, rather than only in the Rust/scan gate.
 */

import { describe, it, expect } from "vitest";

import { ideationApi } from "./ideation";
import { proposalApi } from "./proposal";
import { projectsApi } from "./projects";
import { tasksApi } from "./tasks";

describe("orphan invoke call sites stay deleted", () => {
  it("no proposal-dependency ADD wrapper exists on either surface", () => {
    // `remove` is a real registered command and must survive — asserting its presence is
    // what stops this test from passing because the whole namespace disappeared.
    expect(ideationApi.dependencies.remove).toBeTypeOf("function");
    expect(ideationApi.dependencies).not.toHaveProperty("add");

    expect(proposalApi.removeProposalDependency).toBeTypeOf("function");
    expect(proposalApi).not.toHaveProperty("addProposalDependency");
  });

  it("no session-linking wrappers invoke the HTTP-only handlers", () => {
    // `getChildren` IS a registered command (`get_child_sessions`) and stays.
    expect(ideationApi.sessions.getChildren).toBeTypeOf("function");
    expect(ideationApi.sessions).not.toHaveProperty("createChild");
    expect(ideationApi.sessions).not.toHaveProperty("getParentContext");
  });

  it("no bare delete wrappers exist for projects or tasks", () => {
    expect(projectsApi.archive).toBeTypeOf("function");
    expect(projectsApi).not.toHaveProperty("delete");

    // `cleanupTask` is the supported replacement and must still be there.
    expect(tasksApi.cleanupTask).toBeTypeOf("function");
    expect(tasksApi).not.toHaveProperty("delete");
  });
});
