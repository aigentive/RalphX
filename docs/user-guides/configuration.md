# Project Settings & Configuration User Guide

RalphX is highly configurable — from how tasks execute concurrently to which AI models run your agents. This guide covers every setting, where to find it, and what it does.

---

## Quick Reference

| Question | Answer |
|----------|--------|
| Where are project settings? | Sidebar → Settings (gear icon) for the active project |
| How do I create a new project? | File menu → New Project (or the "+" button in the project switcher) |
| What merge strategy should I use? | **Block** validation + **RebaseSquash** strategy is the safest default |
| How do I use a GitHub PR for plan delivery? | Enable **GitHub PR Mode** — RalphX creates a plan-level PR and waits for GitHub to merge it |
| How do I change my base branch? | Settings → Git → Base Branch (or click "Detect" to auto-detect) |
| Why are my tasks not running? | Check **Max Concurrent Tasks** — may be at the limit, or global cap reached |
| How do I run tests after merges? | Settings → Git → Merge Validation, then configure commands in **Project Analysis** |
| What is the worktree directory? | Where task code lives during execution — never touches your main checkout |
| How do I override agent timeouts? | Edit `ralphx.yaml` in the project root — all timing knobs are there |

---

## Table of Contents

1. [Creating a Project](#creating-a-project)
2. [Git Configuration](#git-configuration)
3. [Merge Settings](#merge-settings)
4. [Execution Settings](#execution-settings)
5. [Review Settings](#review-settings)
6. [Supervisor Settings](#supervisor-settings)
7. [Model Configuration](#model-configuration)
8. [Ideation Settings](#ideation-settings)
9. [Project Analysis](#project-analysis)
10. [Advanced: ralphx.yaml](#advanced-ralphxyaml)
11. [Configuration Reference](#configuration-reference)

---

## Creating a Project

The **Project Creation Wizard** opens the first time you launch RalphX, and any time you add a new project from the project switcher.

### Wizard Steps

1. **Pick a folder** — Click "Browse" to select your git repository's root directory. RalphX reads the folder name and uses it as the default project name.
2. **Name** — Auto-populated from the folder name. Edit to customize.
3. **Base branch** — RalphX auto-detects your repository's default branch (usually `main` or `master`). Override via the dropdown.
4. **Advanced settings (optional)** — Expand to set a custom worktree directory. Defaults to `~/ralphx-worktrees/{project-name}`.

> **Git requirement:** The selected directory must be an initialized git repository. RalphX will show a warning if it can't detect git.

### What Gets Created

| Resource | Created automatically |
|----------|-----------------------|
| Project record | Stored in `ralphx.db` |
| Project slug | Derived from project name (used in branch and worktree names) |
| Default settings | Applied from built-in defaults (overridable in Settings) |
| First analysis | Scheduled — an AI agent scans your project's build system |

---

## Git Configuration

**Location:** Settings → Git

Git settings control how RalphX interacts with your repository during task execution and merging.

### Base Branch

The branch all tasks merge into. Default: `main`.

- Click **Detect** to auto-detect your repository's default branch from `origin`
- Tasks branch from this (or from their plan branch if part of a plan)
- Changing this affects future tasks only — in-progress tasks keep their existing branch targets

### Worktree Location

The parent directory where task worktrees are created. Default: `~/ralphx-worktrees`.

```
~/ralphx-worktrees/
  my-project/
    task-abc123/   ← one worktree per task
    task-def456/
```

> **Your main checkout is never touched.** All agent file operations happen inside the task's worktree.

You can change this to a different disk or SSD if you have disk space concerns, or to a RAM disk for faster I/O. The path must be writable.

### Feature Branches

When enabled, each **plan** gets its own feature branch (e.g., `plan/auth-refactor`). Tasks in the plan merge into the plan branch first, then the plan branch merges to main when all tasks complete.

When disabled, all tasks merge directly to the base branch.

| Mode | Branch flow |
|------|------------|
| Feature branches off | Task branch → base branch (main) |
| Feature branches on | Task branch → plan branch → base branch (main) |

This setting becomes the default for new plans and also reconciles any active plan already in progress.

### Merge Validation

What happens when post-merge validation (tests, lint, typecheck) fails.

| Mode | What happens on failure | When to use |
|------|------------------------|-------------|
| **Block on Failure** (default) | Merge is reverted; task moves to MergeIncomplete | Production projects — ensures only passing code lands |
| **Auto-fix** | An AI agent attempts to fix the failures in the merged code | Projects where most failures are auto-fixable |
| **Warn on Failure** | Merge completes; failures stored as warnings | Non-critical validation (advisory only) |
| **Disabled** | Validation is skipped entirely | Small projects or when testing manually |

Validation commands are configured in [Project Analysis](#project-analysis).

### GitHub PR Mode

GitHub PR Mode changes how RalphX lands a completed **plan** on the base branch.

| Mode | Final plan delivery |
|------|---------------------|
| **Off** | RalphX merges the plan branch directly into the base branch |
| **On** | RalphX creates and monitors a GitHub PR for the plan branch and finishes the task after GitHub merges it |

Important scope rules:

- Only affects the **final plan merge**
- Individual tasks still merge into the plan branch through RalphX's normal merge pipeline
- Requires a GitHub remote and an authenticated `gh` CLI
- New plans inherit the setting automatically
- Active plans are reconciled when you toggle the setting
- If you turn it off mid-plan, RalphX closes the active PR and falls back to the direct-merge path

For the full workflow and UI walkthrough, see the [GitHub PR Mode User Guide](github-pr-mode.md).

---

## Merge Settings

The merge strategy controls _how_ git integrates a task's code into the target branch. Set per-project in **Settings → Git → Merge Validation** (strategy is part of git settings). The four strategies are:

| Strategy | How it works | Git history | Best for |
|----------|-------------|-------------|----------|
| **RebaseSquash** (default) | Rebases onto target, then squashes into one commit | Cleanest linear history | Most projects |
| **Squash** | Combines all commits into one on the target | Linear, one commit per task | Clean history without rebase |
| **Rebase** | Replays commits on top of target; fast-forwards | Linear with individual commits | Preserving per-commit history |
| **Merge** | Creates a merge commit combining source and target | Non-linear (merge commits visible) | Teams requiring full history |

> **Strategy applies project-wide.** All tasks use the same strategy. You can change it at any time — the new strategy applies to future merges only.

For a detailed explanation of the merge pipeline, see the [Merge Pipeline User Guide](merge.md).

---

## Execution Settings

**Location:** Settings → Execution (per-project) and Settings → Global Execution (cross-project)

Execution settings control how many tasks run simultaneously and what happens when things go wrong.

### Per-Project Execution Settings

| Setting | Default | Description |
|---------|---------|-------------|
| **Max Concurrent Tasks** | 10 | Maximum tasks running simultaneously in this project. The UI slider range is 1–10; this is the slider range only, not a system hard limit. Bounded in practice by the global cap. |
| **Auto Commit** | On | Automatically commit uncommitted changes in the worktree after a task completes execution |
| **Pause on Failure** | On | Stop the task queue when any task in this project fails |
| **Review Before Destructive** | On | Insert a review point before tasks that delete files or modify configuration |

### Global Execution Settings

| Setting | Default | Range | Description |
|---------|---------|-------|-------------|
| **Global Max Concurrent** | 20 | 1–50 | Hard cap on total concurrent agents across ALL projects. Tasks wait in Ready when this limit is reached. |

> **How the limits interact:** A task can only start if _both_ its project's `max_concurrent_tasks` limit AND the `global_max_concurrent` limit have available capacity.

### What Consumes a Concurrency Slot

Active agent states each consume one slot:

- Executing, ReExecuting, QaRefining, QaTesting, Reviewing, Merging

States that do **not** consume a slot: Ready, QaPassed, PendingReview, ReviewPassed, Paused, Stopped, Failed.

For a complete explanation of scheduling behavior, see the [Execution Pipeline User Guide](execution.md).

---

## Review Settings

**Location:** Settings → Review

Review settings control the AI code review process that runs after each task completes execution.

| Setting | Default | Description |
|---------|---------|-------------|
| **Enable AI Review** | On | Run an AI reviewer after execution. If disabled, tasks go directly to ReviewPassed (human approval required). |
| **Auto Create Fix Tasks** | On | When the AI reviewer finds issues, automatically create fix tasks and queue them for execution |
| **Require Fix Approval** | Off | Require human approval before executing AI-proposed fix tasks |
| **Require Human Review** | Off | Require your approval even after the AI reviewer approves. Enables a human sign-off step for all tasks. |
| **Max Fix Attempts** | 3 | Maximum times AI can propose fix tasks before the original task is moved to backlog |

### Review Flow

```
Execution complete
    │
    ▼
Reviewing (AI reviewer analyzes code, diff, tests)
    │
    ├── approved    → ReviewPassed → your approval → Approved → Merge
    ├── needs_changes → RevisionNeeded → ReExecuting → (loop)
    └── escalate    → Escalated → your decision required
```

When **Require Human Review** is on, even after AI approval the task stays in ReviewPassed until you click Approve.

---

## Supervisor Settings

**Location:** Settings → Supervisor

The supervisor settings configure the backend watchdog service thresholds used for loop, stall, and runaway-token detection. There is no separate live supervisor agent in the current catalog.

| Setting | Default | Range | Description |
|---------|---------|-------|-------------|
| **Enable Supervisor** | On | On/Off | Enable watchdog monitoring thresholds for execution runs |
| **Loop Threshold** | 3 | 2–10 | Number of identical tool calls before loop detection triggers |
| **Stuck Timeout** | 150 seconds (~2.5 minutes) | 60–1800 | Seconds without git diff progress before stuck detection triggers (5 checks at 30-second intervals) |

### What the Supervisor Does

| Pattern detected | Action |
|-----------------|--------|
| Same tool called N+ times with similar args | Injects guidance into the agent |
| No git diff changes for `stuck_timeout` seconds | Injects guidance or escalates |
| Same error repeating 3+ times | Injects guidance; 4+ times → pauses task |
| Token usage over threshold with no progress | Pauses task and notifies |
| Critical loop detected | Kills agent, analyzes, escalates |

The supervisor is modeled as a service, not a spawned agent, so these settings do not add another execution agent to the lane.

---

## Agent Harness and Model Configuration

**Location:** Settings → General → Execution Agents and Settings → Ideation → Ideation Agents

RalphX supports lane-level harness selection across both the ideation surface and the execution pipeline. Each lane stores its own harness, model, effort, approval-policy, sandbox, and fallback settings.

### Ideation Harness Settings

| Setting | Meaning |
|---------|---------|
| **Harness** | Which runtime to use for the lane, for example `claude` or `codex` |
| **Model** | Harness-native model id |
| **Effort** | Logical reasoning effort for the selected harness |
| **Approval policy** | Harness approval behavior for that lane |
| **Sandbox mode** | Harness sandbox mode for that lane |
| **Fallback harness** | Optional fallback if the preferred harness is unavailable |

### Execution Pipeline Harness Settings

The execution harness screen exposes the same controls for the runtime lanes that ship code:

| Lane | Purpose |
|------|---------|
| **Execution Worker** | Primary task execution lane |
| **Execution Reviewer** | Review lane after execution completes |
| **Execution Re-executor** | Follow-up execution lane after review requests changes |
| **Execution Merger** | Merge-conflict and merge completion lane |

### Current product defaults

| Lane family | Typical default today |
|-------------|-----------------------|
| Ideation lanes | Claude by default, Codex-capable where configured |
| Execution/review/merge lanes | Claude by default, Codex-capable where configured |

### Important limits

| Topic | Current behavior |
|-------|------------------|
| RX-native Team | Provider-neutral; availability follows workflow and harness capabilities |
| Legacy Claude Team mode | Removed; supported coordination now uses RX-native Team |
| Codex execution/review/merge | Supported when those lanes are configured to Codex |
| Legacy session data | Still supported through provider-neutral compatibility fields |

### Recommended rollout

If you are enabling Codex for the first time, use this order:

1. ideation primary
2. ideation verifier
3. execution worker
4. execution reviewer / merger once you are comfortable with that project’s workflow

See [Agent Harnesses](agent-harnesses.md) for the current support matrix and rollout guidance.

---

## Ideation Settings

**Location:** Settings → Ideation

Ideation settings control how RalphX handles implementation planning in the Ideation Studio.

### Plan Workflow Mode

Controls when implementation plans are created relative to task proposals.

| Mode | Behavior |
|------|---------|
| **Required** | A plan must be created before any proposals can be made |
| **Optional** (default) | Plans are suggested for complex features but not required |
| **Parallel** | Plan and proposals are created simultaneously |

### Other Ideation Settings

| Setting | Default | Description |
|---------|---------|-------------|
| **Require explicit approval** | Off | In Required mode: user must click "Approve Plan" before proposals are created |
| **Suggest plans for complex features** | On | In Optional mode: prompts you to create a plan when the feature seems complex |
| **Auto-link proposals to session plan** | On | Automatically sets the plan reference when creating proposals in a session |

---

## Project Analysis

**Location:** Settings → Project Analysis

The Project Analysis section manages the commands RalphX uses to set up and validate task worktrees. This is how RalphX knows to run `npm install` before executing and `cargo clippy` after merging.

### Auto-Detection

When you create a project (or click **Re-analyze**), a `ralphx-project-analyzer` agent scans your repository and detects:
- Build systems (npm, cargo, make, etc.)
- Install commands (e.g., `npm install`, `pip install`)
- Validation commands (e.g., `npm run typecheck`, `cargo test --lib`)

The **Last Analyzed** timestamp shows when detection last ran.

### Customizing Commands

Each detected entry shows as an editable row. You can:
- **Edit commands** inline — click any field to modify it
- **Reset a field** — revert a single customized field to the detected value
- **Add an entry** — manually add a build system entry
- **Remove an entry** — delete an auto-detected or custom entry

Changes are staged locally. Click **Save** in the footer bar to persist them.

**Priority:** When you customize commands by setting `custom_analysis`, it takes **complete precedence** over `detected_analysis`. Your customizations are trusted fully — including explicitly clearing fields (e.g., setting `worktree_setup: []` to skip symlink commands). No automatic merging from detected_analysis occurs into your custom configuration.

### Template Variables

Use these in your command strings:

| Variable | Value |
|----------|-------|
| `{project_root}` | Project working directory |
| `{worktree_path}` | Task worktree directory (available during execution/validation) |
| `{task_branch}` | Task branch name (available during execution) |

### Custom Analysis Format

The analysis is stored as JSON with two sections:

```json
{
  "worktree_setup": [
    { "command": "npm install", "path": ".", "label": "Install dependencies" }
  ],
  "validate": [
    { "command": "npm run typecheck", "path": ".", "label": "Type Check" },
    { "command": "cargo clippy --all-targets", "path": "src-tauri", "label": "Clippy" },
    { "command": "cargo test --lib", "path": "src-tauri", "label": "Tests" }
  ]
}
```

- **`worktree_setup`** — Runs before the worker agent starts (install dependencies, create symlinks). Failures are non-fatal in Warn mode.
- **`validate`** — Runs after a merge to verify code quality. Failures trigger the selected validation mode (Block/AutoFix/Warn/Off).

---

## Advanced: ralphx.yaml

`ralphx.yaml` (in your project root) is the runtime configuration file for all timing, retry limits, and agent behavior. **The UI does not expose these settings** — edit the file directly in a text editor.

All settings support environment variable overrides: `RALPHX_<SECTION>_<FIELD>` (e.g., `RALPHX_RECONCILIATION_MERGER_TIMEOUT_SECS=300`).

### Timeouts

| Setting | Default | Description |
|---------|---------|-------------|
| `timeouts.stream.merge_line_read_secs` | 600 | Max stdout silence before killing the merger agent |
| `timeouts.stream.merge_parse_stall_secs` | 180 | Max stall without parseable events from merger |
| `timeouts.stream.review_line_read_secs` | 300 | Max stdout silence before killing the reviewer agent |
| `timeouts.stream.default_line_read_secs` | 600 | Max silence for worker/ideation agents |
| `timeouts.stream.default_parse_stall_secs` | 180 | Max stall for worker/ideation agents |

### Reconciliation

| Setting | Default | Description |
|---------|---------|-------------|
| `reconciliation.merger_timeout_secs` | 1200 | Deadline for merger agent; triggers MergeIncomplete if exceeded |
| `reconciliation.merging_max_retries` | 3 | Max agent respawns for tasks stuck in Merging |
| `reconciliation.pending_merge_stale_minutes` | 2 | Minutes before a PendingMerge task is considered stale |
| `reconciliation.attempt_merge_deadline_secs` | 120 | Max seconds for a single programmatic merge attempt |
| `reconciliation.validation_deadline_secs` | 1200 | Max seconds for post-merge validation commands |
| `reconciliation.merge_incomplete_max_retries` | 50 | Max auto-retries for MergeIncomplete before surfacing to user |
| `reconciliation.merge_incomplete_retry_base_secs` | 30 | Initial backoff before retrying MergeIncomplete (exponential) |
| `reconciliation.merge_incomplete_retry_max_secs` | 1800 | Cap on MergeIncomplete exponential backoff |
| `reconciliation.validation_revert_max_count` | 2 | Max validation-revert cycles before stopping auto-retry |
| `reconciliation.merge_conflict_max_retries` | 3 | Max auto-retries for MergeConflict |
| `reconciliation.executing_max_retries` | 5 | Max agent respawns for tasks stuck in Executing |
| `reconciliation.executing_max_wall_clock_minutes` | 60 | Wall-clock limit for Executing state |
| `reconciliation.reviewing_max_retries` | 3 | Max agent respawns for tasks stuck in Reviewing |
| `reconciliation.reviewing_max_wall_clock_minutes` | 30 | Wall-clock limit for Reviewing state |
| `reconciliation.qa_max_retries` | 3 | Max agent respawns for QA states |
| `reconciliation.qa_max_wall_clock_minutes` | 15 | Wall-clock limit for QA states |
| `reconciliation.qa_stale_minutes` | 5 | Minutes without QA progress before stale detection |

### Scheduler

| Setting | Default | Description |
|---------|---------|-------------|
| `scheduler.watchdog_interval_secs` | 60 | How often the watchdog scans for stuck tasks |
| `scheduler.ready_settle_ms` | 300 | Milliseconds to wait before scheduling a newly-Ready task |
| `scheduler.merge_settle_ms` | 100 | Milliseconds to wait before unblocking dependents after a merge |

### Git

| Setting | Default | Description |
|---------|---------|-------------|
| `git.cmd_timeout_secs` | 60 | Per-command timeout to prevent hung git processes |
| `git.max_retries` | 3 | Max retries for transient git failures |
| `git.index_lock_stale_secs` | 5 | Age threshold before removing a stale `.git/index.lock` file |
| `git.cleanup_worktree_timeout_secs` | 15 | Timeout for worktree deletion during cleanup |

### Supervisor

| Setting | Default | Description |
|---------|---------|-------------|
| `supervisor.loop_threshold` | 3 | Min repetitions before classifying as a loop |
| `supervisor.stuck_threshold` | 5 | Min stuck-checks before taking action |
| `supervisor.time_threshold_secs` | 600 | Seconds before supervisor warns agent about time usage |
| `supervisor.token_threshold` | 50000 | Token count at which supervisor warns agent |
| `supervisor.max_tokens` | 100000 | Token count at which supervisor forces agent stop |

### Merge Deferral

| Setting | Default | Description |
|---------|---------|-------------|
| `defer_merge_enabled` | `false` | When true: defers merges when conflicts exist or other agents are running. When false: all merges proceed immediately. |

### Limits

| Setting | Default | Description |
|---------|---------|-------------|
| `limits.max_resume_attempts` | 5 | Max times an agent conversation is auto-resumed after a provider error |

---

## Configuration Reference

### Project-Level Settings (UI)

| Setting | Default | Where to change |
|---------|---------|----------------|
| Base branch | `main` | Settings → Git |
| Worktree directory | `~/ralphx-worktrees` | Settings → Git |
| Feature branches | Off | Settings → Git |
| Merge validation mode | `block` | Settings → Git |
| Max concurrent tasks | 10 | Settings → Execution |
| Auto commit | On | Settings → Execution |
| Pause on failure | On | Settings → Execution |
| Review before destructive | On | Settings → Execution |
| Default model | `sonnet` | Settings → Model |
| Allow Opus upgrade | On | Settings → Model |
| AI review enabled | On | Settings → Review |
| Auto create fix tasks | On | Settings → Review |
| Require fix approval | Off | Settings → Review |
| Require human review | Off | Settings → Review |
| Max fix attempts | 3 | Settings → Review |
| Supervisor enabled | On | Settings → Supervisor |
| Loop threshold | 3 | Settings → Supervisor |
| Stuck timeout | 300s | Settings → Supervisor |
| Plan workflow mode | `optional` | Settings → Ideation |
| Require plan approval | Off | Settings → Ideation |
| Suggest plans for complex | On | Settings → Ideation |
| Auto-link proposals | On | Settings → Ideation |
| Validation/install commands | Auto-detected | Settings → Project Analysis |

### Global Settings (UI)

| Setting | Default | Where to change |
|---------|---------|----------------|
| Global max concurrent | 20 | Settings → Global Execution |

### Runtime Settings (ralphx.yaml)

See [Advanced: ralphx.yaml](#advanced-ralphxyaml) above for full tables. Key runtime settings:

| Setting | Default | File location |
|---------|---------|--------------|
| `defer_merge_enabled` | `false` | Root level |
| `reconciliation.merger_timeout_secs` | 1200 | `reconciliation:` section |
| `reconciliation.attempt_merge_deadline_secs` | 120 | `reconciliation:` section |
| `reconciliation.validation_deadline_secs` | 1200 | `reconciliation:` section |
| `reconciliation.executing_max_wall_clock_minutes` | 60 | `reconciliation:` section |
| `scheduler.ready_settle_ms` | 300 | `scheduler:` section |
| `git.cmd_timeout_secs` | 60 | `git:` section |
| `supervisor.max_tokens` | 100000 | `supervisor:` section |
| `limits.max_resume_attempts` | 5 | `limits:` section |

---

## See Also

- [Execution Pipeline](execution.md)
- [Merge Pipeline](merge.md)
- [Agent Orchestration](agent-orchestration.md)
