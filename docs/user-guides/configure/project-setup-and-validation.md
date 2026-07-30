# Teaching RalphX about your project

Tell RalphX how a fresh worktree becomes ready to use and how completed work is checked. At the end, agents can prepare the project, run its validation commands, and make changes against the correct base branch in a predictable worktree location.

**Before you start:** [Installing RalphX and running it for the first time](../01-install-and-first-run.md)

## Set the repository defaults

1. Open Settings → **Repository** → **Repository**.

   This is where RalphX keeps the version-control defaults for the project.

2. Enter the branch you want work to merge into under **Base Branch**.

   Its helper text is “The branch tasks are merged into.”

   Use your normal integration branch, such as `main`, rather than an agent feature branch.

   Click **Detect** if you want RalphX to read the repository's default branch.

   Verify the detected value before leaving the page when your repository uses a nonstandard default.

3. Set **Worktree Location** to the parent directory where RalphX should create task worktrees.

   Its helper text is “Directory where task worktrees are created.”

   Keep this on a local disk with enough free space for several complete checkouts.

   RalphX uses `~/ralphx-worktrees` when no location is set.

   Each task worktree is separate from your primary checkout, so do not point this field at a directory whose contents you do not want RalphX to manage.

## Define setup and validation commands

1. Open Settings → **Repository** → **Setup & Validation**.

   This is where RalphX keeps the detected build-system commands.

2. Click **Refresh Detected Commands** to let RalphX inspect the project.

   The page shows “Not yet analyzed. Click Refresh Detected Commands to detect build systems.” until it has results.

   Review the detected entries instead of assuming they match the way your team builds the project.

3. Expand an entry and review **Path** and **Label** first.

   **Path** identifies the part of the repository the entry applies to.

   **Label** gives that part a recognizable name in the settings page.

   Add separate entries when a monorepo has independently prepared or validated areas.

4. Add the fresh-worktree preparation command under **Install** or **Worktree Setup**.

   **Install** is one optional command for that entry.

   **Worktree Setup** accepts one or more commands RalphX can run after it creates a task worktree.

   Use these fields for commands that install dependencies, generate required local files, or otherwise make a new checkout ready to build.

   Keep the commands safe to run from a clean worktree and specific to the entry's **Path**.

5. Add the checks RalphX should run under **Validate**.

   **Validate** accepts one or more commands.

   Include the build, test, typecheck, lint, or other project checks that make changed work trustworthy for that entry.

   An agent that cannot build or validate the project can still edit files, but its result is unverifiable work.

6. Click **Save** after reviewing every entry.

   RalphX saves your edits as a custom override to detected analysis.

   Use **Reset** beside a changed field, **Reset Entry** for one detected entry, or **Reset All** to return to the detected baseline when needed.

## Keep commands usable in every worktree

1. Prefer commands that need no manual terminal state.

   A worktree may not have dependencies, generated files, or environment-specific setup from your main checkout.

   Put required preparation in **Install** or **Worktree Setup** so an agent has an explicit way to establish that state.

2. Use the available template variables only when a command needs a location.

   **Template Variables** documents `{project_root}`, `{worktree_path}`, and `{task_branch}` in the settings page.

   Use `{worktree_path}` for a command that must run in the task checkout rather than the primary project directory.

3. Re-run **Refresh Detected Commands** after a meaningful build-system change.

   Review the new detection before saving, because your saved custom entries remain the deliberate project-specific configuration.

   Keep setup and validation commands current when dependencies, scripts, or repository layout change.

## What you have now

RalphX knows which branch work should merge into and where to create separate task worktrees. It also has project-specific preparation and validation commands that agents can use from a fresh checkout. That gives agent work a reproducible path to build and verification.

## Next

- [Planning a feature](../workflows/planning-a-feature.md)
