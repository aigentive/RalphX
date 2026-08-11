---
paths:
  - "src-tauri/src/infrastructure/agents/**"
  - "src-tauri/src/application/chat_service/**"
  - "agents/**"
---

# Task Execution Agents

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

**Required Context:** task-state-machine.md | agent-mcp-tools.md | followup-blocker-dedupe.md | multi-harness.md | agent-authoring.md

| Runtime rule | Detail |
|---|---|
| Lane-aware harnesses | Execution/review/merge runtime selection is lane-based even when the current defaults stay Claude-heavy. |
| Harness defaults remain explicit | Worker/reviewer/merger guidance must follow the configured harness and RX-native Team capability contract; do not infer provider behavior from legacy defaults. |
| Validation ownership | Target-project instructions own command selection; execution agents record the narrowest relevant wave/final/re-execution evidence and never manufacture a broad-suite fallback. Broader project/CI automation remains external to the implementation-agent contract. |
| Validation timing | Execution agents do not run baseline validation by default; full baseline task validation and pre-change validation are limited to explicit diagnostics, preconditions, or dry-run command selection. |
| Failure ownership | Final/wave failures on task-scoped or modified surfaces must be fixed; unrelated pre-existing failures are reported or registered without silently expanding the current task. |

---

## Worker (`ralphx-execution-worker`)

| Aspect | Detail |
|--------|--------|
| **Model** | sonnet |
| **Trigger** | `executing` or `re_executing` entry |
| **CWD** | Worktree path or project dir |
| **Permission** | `acceptEdits` (Write/Edit/Bash pre-approved) |
| **Bootstrap** | `<task_runtime_context>` carries task id/state/project/worktree; `RALPHX_TASK_STATE` remains backend-owned fallback (`executing` or `re_executing`) |

**Execution flow:**
1. If `re_executing` from `<task_runtime_context>` or fallback env → fetch `get_review_notes()` + `get_task_issues(status: "open")` before code changes
2. Use `<task_runtime_context>` as bootstrap context only; it is not final authority for blockers, stale state, scope drift, plan details, or completion readiness
3. Call `get_task_context(task_id)` when bootstrap context is absent, blocked, stale/incomplete, or full task/proposal/plan/scope details are needed
4. If blocked → STOP
5. Read plan artifact; apply the orchestration pattern below when decomposing and delegating:

   <!-- Inlined from docs/architecture/system-card-orchestration-pattern.md (§2 §4 §5 §9) -->
   **Execution phases:** Discovery → Plan Design (dependency graph + wave schedule) → Wave execution → Commit gate → repeat → Verify & clean up

   **Conflict Prevention Rules (NON-NEGOTIABLE):**
   | # | Rule |
   |---|------|
   | 1 | **File ownership** — each coder writes exclusive files; no two agents modify the same file per wave |
   | 2 | **Create-before-modify** — new files first; agent crash can't corrupt existing code |
   | 3 | **Commit gates** — each wave ends with a verified commit; next wave only after committed |
   | 4 | **Read-only sources** — agents read existing files for reference but only modify scoped files |
   | 5 | **No cascading deletes** — delete only after replacements verified working |

   **STRICT SCOPE template for coder dispatches:**
   ```
   STRICT SCOPE:
   - You may ONLY create/modify: [file list]
   - You must NOT modify: [exclusion list]
   - Read for reference only: [reference file list]

   TASK: [specific deliverable]

   TESTS: Write tests for new code. Do NOT modify existing test files.
   VERIFICATION: Run [lint command] on modified files only.
   ```

   **Typical execution sequence:** `Read → Write/Edit → Bash (typecheck + test) → Grep (verify no dead refs) → commit`

6. Decompose task into sub-scopes, build dependency graph, schedule waves
7. Delegate to `ralphx-execution-coder` instances (max 3 concurrent, no overlapping write files)
8. Apply wave gates (validate each wave before starting next)
9. `start_step()` → work → `complete_step()` (per step)
10. For re-execution: `mark_issue_in_progress()` / `mark_issue_addressed()` per issue

**Key MCP tools:** `start_step`, `complete_step`, `skip_step`, `fail_step`, `add_step`, `get_task_context`, `get_review_notes`, `get_task_issues`, `mark_issue_in_progress`, `mark_issue_addressed` (+ `delegate_start` / `delegate_wait` / `delegate_cancel` for coder delegation)

## Reviewer (`ralphx-execution-reviewer`)

| Aspect | Detail |
|--------|--------|
| **Model** | sonnet |
| **Trigger** | `reviewing` entry |
| **Session** | Always fresh (never resumed) |

**MUST call `complete_review` before exiting.** Task stuck in `reviewing` otherwise.

**`complete_review` params:**
- `decision`: `"approved"` | `"needs_changes"` | `"escalate"` | `"approved_no_changes"`
- `feedback` (required)
- `issues[]` (REQUIRED for needs_changes): `{ title, severity, step_id, description, file_path, line_number }`
- `escalation_reason` (if escalate)

**Review outcomes → transitions:**

| Outcome | Transition |
|---------|------------|
| `approved` | `reviewing` → `review_passed` |
| `needs_changes` | `reviewing` → `revision_needed` → (auto) `re_executing` |
| `escalate` | `reviewing` → `escalated` |

`approved` never means direct `reviewing → approved`. When `require_human_review=false`, backend approval continues from `review_passed → approved`. `approved_no_changes` is the separate no-code / skip-merge path.

## Merger (`ralphx-execution-merger`)

| Aspect | Detail |
|--------|--------|
| **Model** | opus (most capable, for complex conflicts) |
| **Trigger** | `merging` entry (after programmatic merge fails) |
| **Pre-approved** | Read, Edit, Bash |

**Merger workflow:**
1. `get_merge_target(task_id)` → returns `{ source_branch, target_branch }`
2. `get_task_context(task_id)` → conflict files, task details
3. Read each conflicted file, resolve markers, edit files
4. Verify: grep for remaining `<<<<<<< HEAD`, then run the project-specific validation commands from `get_project_analysis()`
5. Stage only the resolved files, then `git rebase --continue` if a rebase is active or create the required merge/recovery commit

**Merger MCP tools:**

| Tool | Purpose | Required? |
|------|---------|-----------|
| `get_merge_target` | Get source/target branches | YES (always call first) |
| `report_conflict` | Cannot resolve → `MergeConflict` | YES if stuck |
| `report_incomplete` | Non-conflict failure → `MergeIncomplete` | YES if git error |
| `get_task_context` | Task details + conflict files | As needed |

---

## ChatService Context → Agent Resolution

| Context Type | Default Agent | Status Override | Session |
|-------------|---------------|-----------------|---------|
| `TaskExecution` | `ralphx-execution-worker` | — | Never resumed (fresh spawn) |
| `Review` | `ralphx-execution-reviewer` | `review_passed` → `ralphx-review-chat`, `approved` → `ralphx-review-history` | Never resumed (fresh) |
| `Merge` | `ralphx-execution-merger` | — | May resume |
| `Ideation` | `ralphx-ideation` | `accepted` → `ralphx-ideation-readonly` | Resumes |
| `Task` | `ralphx-chat-task` | — | Resumes |
| `Project` | `ralphx-chat-project` | — | Resumes |

## Support Agents

| Agent | Model | Role |
|-------|-------|------|
| `ralphx-ideation` | opus | Facilitates ideation sessions, creates task proposals + plans |
| `ralphx-utility-session-namer` | haiku | Generates 2-word session titles |
| `ralphx-chat-task` | sonnet | Task-specific Q&A |
| `ralphx-chat-project` | sonnet | Project-level questions |
| `ralphx-review-chat` | sonnet | Discuss review findings (when status = `review_passed`) |
| `ralphx-qa-prep` | sonnet | Generate acceptance criteria + test steps (background, on `ready`) |
| `ralphx-qa-executor` | sonnet | Browser-based QA via agent-browser |
| `ralphx-execution-orchestrator` | opus | Complex multi-step coordination |
| `ralphx-research-deep-researcher` | opus | Thorough research |

---

## Key Files Index

| Component | Path |
|-----------|------|
| GitMode enum | `src-tauri/crates/ralphx-domain/src/entities/project.rs` |
| InternalStatus (28 variants) | `src-tauri/crates/ralphx-domain/src/entities/status.rs` |
| Valid transitions table | `src-tauri/crates/ralphx-domain/src/entities/status.rs:valid_transitions()` |
| TaskEvent enum | `src-tauri/src/domain/state_machine/events.rs` |
| State machine dispatcher | `src-tauri/src/domain/state_machine/machine/transitions.rs` |
| TransitionHandler + auto-transitions | `src-tauri/src/domain/state_machine/transition_handler/mod.rs` |
| on_enter side effects | `src-tauri/src/domain/state_machine/transition_handler/side_effects/mod.rs` |
| GitService (all git ops) | `src-tauri/src/application/git_service/` |
| TaskTransitionService | `src-tauri/src/application/task_transition_service.rs` |
| Task scheduler | `src-tauri/src/application/task_scheduler_service/` |
| PlanBranch entity | `src-tauri/crates/ralphx-domain/src/entities/plan_branch.rs` |
| PlanBranch repo trait | `src-tauri/crates/ralphx-domain/src/repositories/plan_branch_repository.rs` |
| Agent configs (three-layer allowlist) | `src-tauri/src/infrastructure/agents/claude/agent_config/` |
| Agent spawner (CWD resolution) | `src-tauri/src/infrastructure/agents/spawner.rs` |
| ChatService contexts | `src-tauri/src/application/chat_service/chat_service_context.rs` |
| HTTP merge handlers | `src-tauri/src/http_server/handlers/git.rs` |
| Canonical agent definitions | `agents/*/agent.yaml` + prompt files |
| Plan branch commands | `src-tauri/src/commands/plan_branch_commands.rs` |
| Ideation apply | `src-tauri/src/commands/ideation_commands/ideation_commands_apply.rs` |
| Frontend plan-branch API | `frontend/src/api/plan-branch.ts` |
| Frontend GitMode type | `frontend/src/types/project.ts` |
