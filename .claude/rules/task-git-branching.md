---
paths:
  - "src-tauri/src/application/git_service/**"
  - "src-tauri/crates/ralphx-domain/src/entities/project.rs"
  - "src-tauri/crates/ralphx-domain/src/entities/plan_branch.rs"
  - "src-tauri/crates/ralphx-domain/src/repositories/plan_branch_repository.rs"
  - "src-tauri/src/domain/state_machine/transition_handler/merge_helpers.rs"
  - "src-tauri/src/http_server/handlers/git.rs"
  - "src-tauri/src/commands/plan_branch_commands.rs"
  - "src-tauri/src/commands/ideation_commands/**"
  - "frontend/src/api/plan-branch.ts"
  - "frontend/src/components/projects/ProjectCreationWizard/**"
  - "frontend/src/types/project.ts"
---

# Task Git Branching & Merge

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

**Required Context:** task-state-machine.md | agent-mcp-tools.md

---

## Git Mode: Worktree (only mode)

`GitMode::Worktree` is the sole variant (`crates/ralphx-domain/src/entities/project.rs` — serde alias `"local"` accepted for legacy rows; migration `v44_remove_local_git_mode.rs` converted all legacy local projects).

| Aspect | Behavior |
|---|---|
| **Isolation** | Separate worktree directory per task; unlimited parallel tasks |
| **Agent CWD** | `task.worktree_path` |
| **Cleanup** | Delete worktree + branch on merge |
| **DB fields** | `task.task_branch` + `task.worktree_path` |

**Config:** `project.base_branch` (default: `"main"`) + `project.worktree_parent_directory` (default: `~/ralphx-worktrees`)

---

## Branch Hierarchy (Two Levels)

```
main (project.base_branch)
 ├─ ralphx/{slug}/plan-{artifact-id-8chars}     ← plan feature branch
 │   ├─ ralphx/{slug}/task-{task-id}            ← task branch (merges → plan branch)
 │   └─ [merge task] plan branch → main         ← final plan merge
 └─ ralphx/{slug}/task-{task-id}                ← standalone task (merges → main)
```

### Branch Naming

| Type | Pattern | Example |
|------|---------|---------|
| Task branch | `ralphx/{project-slug}/task-{task-id}` | `ralphx/my-app/task-abc123` |
| Plan branch | `ralphx/{project-slug}/plan-{short-artifact-id}` | `ralphx/my-app/plan-a1b2c3d4` |
| Worktree path | `{parent}/{project-slug}/task-{task-id}` | `~/ralphx-worktrees/my-app/task-abc123` |

`slugify()`: lowercase, non-alphanumeric → `-`, trim dashes

### Feature Branches (Plan-Level)

**Toggle:** `project.use_feature_branches` (default: `true`)

**Created at:** Plan apply (`apply_proposals_to_kanban`) or mid-plan (`enable_feature_branch`)

**On creation:**
1. Git branch `ralphx/{slug}/plan-{id}` from `project.base_branch`
2. DB record in `plan_branches` table (status: `Active`)
3. Auto-create merge task (status: `Blocked`, category: `plan_merge`)
4. Merge task `blockedBy` all plan tasks

**Entity:** `PlanBranch { id, plan_artifact_id, session_id, project_id, branch_name, source_branch, status, merge_task_id }`

**Status:** `Active` → `Merged` | `Abandoned`

### Task / Session / PlanBranch Data Model

```
IdeationSession (has plan proposals)
  ├─ task.ideation_session_id → always set (canonical session link)
  ├─ task.plan_artifact_id    → set ONLY when real artifact exists (FK to artifacts table)
  └─ plan_branches.session_id → UNIQUE index, primary lookup key
```

| Field | Always Set? | FK Constraint? | Use For |
|-------|-------------|----------------|---------|
| `task.ideation_session_id` | YES (if from session) | None | Plan branch lookups, graph grouping |
| `task.plan_artifact_id` | Only if plan artifact exists | YES `REFERENCES artifacts(id)` | Artifact content retrieval |
| `plan_branches.session_id` | YES | None (UNIQUE index) | Primary plan branch lookup |
| `plan_branches.plan_artifact_id` | YES (may be session fallback) | None | Legacy compat |

**Rule:** Never put a session UUID into `task.plan_artifact_id` — FK violation. Use `ideation_session_id` instead.

### Base Branch Resolution

**File:** `merge_helpers.rs:resolve_task_base_branch()`

| Condition | Base Branch |
|-----------|-------------|
| Task has `ideation_session_id` AND plan has active feature branch | Plan feature branch |
| Otherwise | `project.base_branch` (default: `"main"`) |

### Merge Target Resolution

**File:** `merge_helpers.rs:resolve_merge_branches()`

| Condition | Source → Target |
|-----------|-----------------|
| Task IS the merge task (`plan_branches.merge_task_id`) | Plan feature branch → project base |
| Task belongs to plan with active feature branch (via `ideation_session_id`) | Task branch → plan feature branch |
| Standalone task (no plan) | Task branch → project base |

**Lookup path:** `task.ideation_session_id` → `plan_branch_repo.get_by_session_id()`.

### GitHub PR Mode Plan PR Freshness

**Files:** `pr_merge_poller.rs` + `pr_startup_recovery.rs` + `task_transition_service.rs` + `on_enter_states/merge.rs`

| Condition | Required behavior |
|-----------|-------------------|
| Open RX-managed plan PR is behind base and mergeable | Programmatically update plan branch from remote base, push plan branch, refresh PR metadata, stay `WaitingOnPr` |
| Open RX-managed plan PR is dirty/conflicting | Set `pr_branch_update_conflict=true`, spawn merger agent, return to `WaitingOnPr` after `complete_merge` |
| `Merging` has `pr_branch_update_conflict=true` | Bypass PR-poller shortcut; merger agent must spawn |
| App restarts while PR is open | Startup recovery checks PR freshness before restarting poller; independent PR tasks may reconcile with bounded parallelism |

**Invariant:** PR freshness work updates the PR branch only; GitHub remains final merge authority, so this path must not mark the plan merge task `Merged`.
**Worktree invariant:** PR freshness must never merge directly inside `project.working_directory`; use isolated worktrees and refuse background repair if the plan branch is checked out in the primary repo.

---

## Merge Workflow (Two-Phase)

### Phase 1: Programmatic (Fast Path)

**Triggered on:** `pending_merge` entry via `attempt_programmatic_merge()`

Merge strategies run in **temporary isolated worktrees** (`compute_rebase_worktree_path` / `compute_merge_worktree_path` in `merge_strategies.rs`) or via checkout-free plumbing (`checkout_free_*` in `git_service/checkout_free.rs`) — never by checking out branches in the primary repo.

| Step | Action |
|------|--------|
| 1 | Resolve source/target via `resolve_merge_branches()` |
| 2 | Delete task worktree first (unlock branch) |
| 3 | `GitService::try_rebase_and_merge_in_worktree()` (`git_service/merge.rs`) — rebase+merge inside a temp worktree; fallback `try_merge_in_worktree()` — plain merge inside a temp worktree |
| 4a | **Success** → `complete_merge_internal()` → `Merged` |
| 4b | **Conflict** → transition to `Merging` → spawn merger agent |
| 4c | **Error** → transition to `MergeIncomplete` (human-waiting) |

**`complete_merge_internal()` cleanup:**
- Persist `task.merge_commit_sha`
- Delete worktree
- Delete task branch
- For plan merge tasks: mark `plan_branch.status = Merged`, delete feature branch
- Emit `merge:completed` + `task:status_changed`

### Phase 2: Agent-Assisted (Conflict Resolution)

**Triggered on:** `merging` entry — spawns **merger agent** (opus model). See task-execution-agents.md.

**Merge outcome detection (auto, on agent exit):**

| Condition | Result |
|-----------|--------|
| No rebase in progress + no conflict markers | Auto → `Merged` |
| Rebase still in progress or conflict markers found | Auto → `MergeConflict` |

### Phase 3: Manual (Human Resolution)

| From | Event | → To |
|------|-------|------|
| `merge_conflict` | `ConflictResolved` | `merged` |
| `merge_incomplete` | `Retry` | `pending_merge` (re-attempt programmatic merge) |
| `merge_incomplete` | `MergeConflict` | `merging` (spawn agent) |
| `merge_incomplete` | `ConflictResolved` | `merged` |

---

## Git Operations (GitService)

**Module:** `src-tauri/src/application/git_service/` (`branch.rs`, `merge.rs`, `worktree.rs`, `commit.rs`, `rebase.rs`, `state_query.rs`, `checkout_free.rs`) — stateless, all methods static.

### Branch Ops

| Method | Git Command |
|--------|-------------|
| `create_branch(repo, branch, base)` | `git branch {branch} {base}` |
| `checkout_branch(repo, branch)` | `git checkout {branch}` |
| `delete_branch(repo, branch, force)` | `git branch -d/-D {branch}` |
| `create_feature_branch(repo, branch, source)` | `git branch {branch} {source}` (no checkout) |
| `delete_feature_branch(repo, branch)` | `git branch -d {branch}` |
| `get_current_branch(repo)` | `git rev-parse --abbrev-ref HEAD` |

### Worktree Ops

| Method | Git Command |
|--------|-------------|
| `create_worktree(repo, path, branch, base)` | `git worktree add -b {branch} {path} {base}` |
| `delete_worktree(repo, path)` | `git worktree remove --force {path}` |

### Commit Ops

| Method | Git Command |
|--------|-------------|
| `commit_all(path, msg)` | `git add -A && git commit -m {msg}` → returns SHA |
| `has_uncommitted_changes(path)` | `git status --porcelain` |
| `get_head_sha(path)` | `git rev-parse HEAD` |

### Merge/Rebase Ops

| Method | Git Command | Returns |
|--------|-------------|---------|
| `merge_branch(repo, source, _target)` | `git merge {source} --no-edit` | `Success` / `FastForward` / `Conflict` |
| `rebase_onto(path, base)` | `git rebase {base}` | `Success` / `Conflict` |
| `abort_merge(repo)` | `git merge --abort` | — |
| `abort_rebase(path)` | `git rebase --abort` | — |
| `get_conflict_files(repo)` | `git diff --name-only --diff-filter=U` | File list |

### Merge State Detection

| Method | Checks |
|--------|--------|
| `is_rebase_in_progress(worktree)` | `.git/rebase-merge` or `.git/rebase-apply` dirs |
| `has_conflict_markers(worktree)` | Scans tracked files for `<<<<<<<` |
| `is_commit_on_branch(repo, sha, branch)` | `git merge-base --is-ancestor` |

---

## Conflict Resolution Patterns

### Duplicate Migrations

**Pattern:** Task branch and plan branch both add migration version N (same table name, same structure).

**Root cause:** Task created off main before plan branch integrated earlier migration work. On rebase, both try to add v33.

**Resolution:**
1. Do not hand-pick the next integer migration version
2. Regenerate the task-branch migration with `python3 scripts/new_sqlite_migration.py <description>` after rebasing on latest `main`
3. Keep the already-shipped migration ids on the target branch untouched
4. Run `python3 scripts/validate_sqlite_migrations.py` before continuing the rebase or merge
5. Adapt task-branch-specific entity/repo methods to the plan branch's type definitions (don't change types mid-rebase)

**Files:** `src-tauri/src/infrastructure/sqlite/migrations/`

### Type Definition Conflicts (IDs, Entities)

**Pattern:** Task branch uses String-based ID newtype but plan branch uses Uuid-based newtype in the domain crate.

**Root cause:** Competing approaches to type safety. Plan branch integrates domain types first, task branch adds surface-layer types.

**Resolution:**
1. Keep plan branch's type definition (it's already deployed)
2. Adapt task branch's new methods to use the plan branch's type
3. Never change types during rebase — preserve both approaches:
   - Domain layer: newtypes in `crates/ralphx-domain/src/entities/`
   - Conversion happens only at API boundaries (HTTP handlers)

**Files:** `src-tauri/crates/ralphx-domain/src/entities/`, `src-tauri/crates/ralphx-domain/src/repositories/`

### Multi-Commit Rebase Strategy

**Pattern:** Task branch has 2+ commits. First commit creates conflicts (e.g., migrations), second commit has entity/repo conflicts.

**Strategy:**
1. Resolve first commit's conflicts in isolation (read all conflicted files for that commit)
2. `git add <file> && git rebase --continue` → rebase moves to next commit
3. Repeat for each commit until completion
4. Later commits may rebase cleanly if they don't conflict

**Commands:**
```bash
git rebase <target-branch>
# Conflict 1
git add <resolved-files>
git rebase --continue
# Conflict 2 (if any)
git add <resolved-files>
git rebase --continue
```
