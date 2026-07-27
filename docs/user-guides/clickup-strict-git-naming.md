# Strict ClickUp Git Naming User Guide

Strict ClickUp Git naming keeps every RalphX Agent workspace for a ClickUp task on one stable, ticket-shaped branch. It also enforces the commit subjects and pull request title that belong to that task.

> **Rollout status for PR #774:** the backend policy, persistence, branch lifecycle, publish enforcement, recovery, and tests are implemented. The user-facing Settings editor, start preview, managed-workspace summary, and publish-policy panels are still open work. In the current branch, Settings -> Integrations -> ClickUp only exposes the API token and workspace selector, so there is not yet a supported UI path to enable this feature. The walkthrough below describes the completed behavior and the UI flow users should expect when the remaining frontend phases land.

---

## Quick Reference

| Question | Answer |
|----------|--------|
| What problem does this solve? | It prevents ticket work from drifting across arbitrary branches, commit names, and PR titles |
| Where will I configure it? | Settings -> Integrations -> ClickUp -> Git naming convention |
| Is it enabled by default? | No. It is opt-in |
| Which Agent modes use it? | Ticket-backed Edit, Plan, and Ideation workspaces |
| Does Chat create a branch? | No. A branchless Chat remains branchless until it switches to a workspace-owning mode |
| Can two conversations work on the same ticket branch at once? | No. The backend blocks the second workspace and returns the active owner; the planned UI provides an open-owner action |
| What happens after a PR merges? | RalphX safely prepares the same frozen branch for the next cycle |
| Do later template or ClickUp title changes rename an existing branch? | No. The first binding is frozen |

---

## User Story

> As a developer starting RalphX work from a ClickUp task, I want the task to determine the branch, commit subject rule, and PR title so that GitHub activity links back to ClickUp and every conversation uses the same project convention.

Example ClickUp task:

| Field | Value |
|-------|-------|
| Task ID | `CU-123` |
| Task name | `Fix Login / Redirect` |
| Authenticated ClickUp user | `Ada Lovelace` |

With the default templates, RalphX freezes:

```text
Branch:        cu-123_fix-login-redirect_ada-lovelace
Commit:        CU-123 - Fix Login / Redirect
PR title:      CU-123 - Fix Login / Redirect
```

The branch value is normalized into a Git-safe lowercase name. The commit subject and PR title preserve readable ticket text.

---

## Workflow at a Glance

```text
Settings -> Integrations -> ClickUp
                 |
                 v
       Connect and validate ClickUp
                 |
                 v
       Enable strict Git naming
                 |
                 v
       Edit templates + preview
                 |
                 v
      Save for new ticket bindings
                 |
                 v
        Select a ClickUp task
                 |
                 v
       Start Edit / Plan / Ideation
                 |
                 v
   RalphX fetches the task and current user
                 |
                 v
  Existing frozen binding? -- yes --> reuse it
                 |
                no
                 v
  Render + persist branch/commit/PR convention
                 |
                 v
   Check branch, PR, and active-owner evidence
                 |
                 v
        Create or reuse exact branch
                 |
                 v
      Work -> validate commits -> publish PR
                 |
                 v
       PR merged and workspace cleaned
                 |
                 v
       Safely reuse the same branch
```

RalphX fails closed when it cannot prove that the requested branch is safe to create, own, publish, or reuse.

---

## Before You Use It

You need:

- A connected and validated ClickUp integration.
- A selected ClickUp workspace.
- A Git project with an `origin` remote.
- A ClickUp task attached to the Agent conversation.
- Edit, Plan, or Ideation mode when the conversation needs a worktree.

If the branch template contains `:username:`, RalphX must be able to resolve the authenticated ClickUp user. The task creator or assignee is not used as a substitute.

---

## Step-by-Step Setup

### 1. Connect ClickUp

1. Open **Settings**.
2. Select **Integrations**, then **ClickUp**.
3. Enter a ClickUp personal API token.
4. Select **Save API token**.
5. Select **Validate** and confirm that task references are enabled.
6. Select the ClickUp workspace that contains your tasks.

What to expect:

- RalphX stores the token in the system credential store rather than returning it to the UI.
- The panel shows whether the token is stored, validation is successful, task search is available, and a workspace is selected.

### 2. Enable strict Git naming

After the frontend rollout is complete:

1. Expand **Git naming convention** in the ClickUp settings panel.
2. Turn on **Strict Git naming**.
3. Review the three default templates.

```text
Branch name:    :taskId:_:taskName:_:username:
Commit subject: :taskId: - :taskName:
PR title:       :taskId: - :taskName:
```

The setting affects new ticket bindings. A ticket that already has a frozen strict binding keeps it even if the global toggle is later disabled.

### 3. Customize templates, if needed

Supported placeholders:

| Placeholder | Branch | Commit | PR title | Value |
|-------------|--------|--------|----------|-------|
| `:taskId:` | Yes | Yes | Yes | ClickUp custom ID when available, otherwise the task ID |
| `:taskName:` | Yes | Yes | Yes | Authoritative ClickUp task name |
| `:username:` | Yes | Yes | Yes | Authenticated ClickUp user |
| `:summary:` | No | Yes | Yes | Dynamic commit text; the frozen PR rendering uses the task-title snapshot |

Rules:

- Every template must contain `:taskId:`.
- Unknown placeholders are rejected.
- A branch template cannot contain `:summary:` because the branch must stay stable.
- A commit or PR template may contain `:summary:` once.
- Empty or invalid rendered Git branch parts are rejected.
- Long branch names are shortened deterministically to remain Git-safe.

Example with dynamic commit summaries:

```text
Commit template: :taskId: - :summary:

Valid commit:     CU-123 - Add redirect validation
Invalid commit:   Fix redirect validation
```

### 4. Review the preview and save

The planned preview refetches the ClickUp task and current user, then shows the branch, commit rule/example, and PR title before you save or start work.

What to expect:

- The preview is advisory; submit-time backend validation remains authoritative.
- Changing the selected project or task invalidates an older preview.
- Saving a template does not rename a previously bound ticket branch.
- Validation errors stay attached to the field that needs correction.

### 5. Start work from a ClickUp task

1. Open an Agent conversation entry point.
2. Select the project and ClickUp task.
3. Choose Edit, Plan, or Ideation mode.
4. Choose the PR target/base branch when needed.
5. Review the managed branch preview.
6. Start the conversation.

RalphX then:

1. Refetches the authoritative ClickUp task.
2. Resolves the authenticated ClickUp user if the template needs it.
3. Loads an existing frozen binding or creates one atomically.
4. Checks local branches, remote branches, open PRs, and active workspace ownership.
5. Creates or reuses the exact managed branch.
6. Creates the worktree in RalphX's hashed workspace location.
7. Installs an early commit-message check in the managed worktree.

The selected PR base controls where the work will merge. It does not replace or rename the managed head branch.

---

## What to Expect While Working

### One active owner

Only one active Agent conversation can own a strict ticket branch. If another conversation already owns it, RalphX blocks the new start and identifies the owner conversation so the UI can open it.

This is an ownership rule, not a naming workaround. RalphX does not append a conversation ID or concurrency suffix.

### Frozen policy

The first successful binding freezes:

- The rendered branch name.
- The task title snapshot.
- The ClickUp username snapshot.
- The commit subject rule.
- The PR title.
- The policy version.

Later task renames, account changes, and template edits affect new bindings only.

### Branchless Chat

A Chat conversation remains branchless. If it is linked to a ClickUp task and later switches to Edit or Plan, RalphX recovers the ticket identity and applies the same strict binding during that mode switch.

### Commits

The managed worktree checks commit subjects early, but the Git hook is only a convenience. Before any push or PR side effect, RalphX validates every commit introduced during the current ticket cycle.

- A local, unpublished mismatch can enter the workspace repair flow.
- A mismatch already present in published history requires operator action.
- `git commit --no-verify` does not bypass publish-time validation.

### Pull requests

RalphX creates or updates the PR with the frozen title, even if generated conversation text suggests another title. The normal reviewer-focused PR body and repository template behavior remain unchanged.

---

## After the Pull Request

### Merged PR

After GitHub reports the PR merged and RalphX completes guarded cleanup, the exact same ticket branch can begin another cycle.

Before reuse, RalphX verifies that:

- The previous PR really merged.
- The workspace is clean.
- The old branch content is merged or content-equivalent to the target branch.
- No unpublished local or remote work would be discarded.
- No conflicting or unmanaged worktree owns the branch.

RalphX then advances or safely recreates the clean branch without force-pushing and records a new cycle base.

### Closed without merge

A PR closed without merge does not qualify for automatic reuse. RalphX blocks the next cycle so unpublished or divergent work cannot be silently discarded.

---

## Common Blockers

| Situation | What RalphX does | What you should do |
|-----------|------------------|--------------------|
| Template is invalid or misses `:taskId:` | Blocks save/start | Correct the highlighted template |
| `:username:` is required but the current ClickUp user cannot be resolved | Blocks start | Revalidate ClickUp credentials or remove `:username:` from the template |
| Another conversation owns the branch | Blocks the second start and returns the owner ID | Open and continue the owning conversation |
| Existing branch or open PR does not match the frozen convention | Blocks start | Inspect the named branch/PR and resolve the mismatch explicitly |
| Legacy binding cannot be safely adopted | Blocks start | Finish or clean up the legacy ticket work before migration |
| Workspace is on the wrong branch | Blocks publish | Restore the exact frozen branch |
| Local commit subject violates the rule | Starts repair when safe | Let the repair complete, then retry publish |
| Published commit subject violates the rule | Blocks automatic repair | Resolve the remote history through an explicit operator decision |
| Prior PR closed without merge | Blocks branch reuse | Recover or close out the old work without discarding it |
| Branch is dirty, divergent, or contains unpublished work | Blocks cleanup/reuse | Preserve and reconcile the outstanding work manually |

---

## Turning the Feature Off

Turning strict naming off changes new ticket bindings only.

- Existing frozen bindings remain authoritative.
- Existing managed branches are not renamed or orphaned.
- Disconnecting ClickUp disables enforcement for new bindings but preserves the saved template strings for a later reconnect.
- Non-strict ClickUp workspaces keep RalphX's existing isolated branch behavior.

---

## Current Implementation Checklist

| Capability | PR #774 status |
|------------|----------------|
| Settings persistence and template validation | Implemented in backend |
| Immutable ticket binding | Implemented |
| Strict start and one-owner enforcement | Implemented |
| Commit and frozen PR-title publish gate | Implemented |
| Recovery, cleanup, and same-branch reuse | Implemented |
| ClickUp settings template editor | Not yet implemented |
| Start preview and structured blocker actions | Not yet implemented |
| Managed workspace/header/publish summaries | Not yet implemented |
| Final native UI smoke and end-to-end guide refresh | Pending after frontend phases |

Until the frontend items are complete, treat this document as the usage contract and acceptance guide rather than an indication that the toggle is available in the current app build.

---

## Related Guides

- [Project Settings and Configuration](configuration.md)
- [Execution and Monitoring](execution.md)
- [GitHub PR Mode](github-pr-mode.md)
- [Merge Pipeline](merge.md)
