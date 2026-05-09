---
name: ralphx-swe
description: >
  RalphX autonomous agent playbook. Teaches external agents pipeline navigation:
  ideation → execution → review → merge. Covers 24-state machine, decision trees
  for edge cases, event-driven patterns, failure recovery playbooks.
  Use when connecting to RalphX via External MCP API or managing pipeline tasks.
argument-hint: "[section: quick-start | state-machine | decisions | events | recovery | dos-donts | cross-project]"
---

# RalphX SWE Skill — Autonomous Pipeline Playbook

> **Skill vs Guide:** Call `v1_get_agent_guide` for tool schemas and sequencing rules. This skill
> teaches **judgment**: when to act vs observe, how to handle edge cases, what events mean for
> decision-making. Zero overlap with the guide's API reference content.

---

## 1. Bootstrap — Start Here Every Session

Two steps before taking any pipeline action:

**Canonical tool naming** (read this before any tool call):
```
Examples in this skill use canonical RalphX method names: v1_*

Host mappings:
- RalphX external MCP methods: v1_*
- Claude/Codex MCP wrappers in some environments: mcp__ralphx__v1_*
- ReefBot integration: ralphx__v1_*

Never invent a namespace from this skill name.
Never call mcp__ralphx-swe__*.
```

**Step 1 — Check attention items** (find what needs action right now):
```
v1_get_attention_items(project_id)
→ Returns escalated_reviews, failed_tasks, merge_conflicts.
→ Address these before doing anything else.
```

**Step 2 — Load tool reference** (for argument schemas and preconditions):
```
v1_get_agent_guide()  → or v1_get_agent_guide(section: "pipeline") for supervision tools
```

---

## 2. Core Principles

**Observe before act.** Most pipeline states are agent-active (executing, reviewing, merging). Your
role in those states is to watch, not interrupt. Only take action in waiting states
(`review_passed`, `escalated`, `qa_failed`, `merge_conflict`, `merge_incomplete`).

**Annotate before you intervene.** Before calling any action tool, call `v1_create_task_note` with
your reasoning. This creates an audit trail for humans reviewing your decisions.

**Always pair UUIDs with titles.** Any task ID you surface in a log, message, or annotation must
include the human-readable title. Use `v1_get_task_detail` or `v1_batch_task_status` to resolve
titles. Format: `task-{id} ({Title})`.

**Webhook chat updates should stay human-first.** Machine IDs from RalphX webhook events are
stored separately in message/session metadata. In normal user-facing chat updates, prefer human
titles and project labels; do not dump raw UUID footers unless the user explicitly asks for them.

**Separate review approval from merge progression.** `review_passed` is the approval-decision
point. `merge:ready` / `pending_merge` are merge-pipeline observation states. Do NOT ask for a
second "merge approval" unless your integration explicitly defines one.

**Approval authority is integration-specific.** On `review_passed`, either:
- act if your current policy/delegation allows `v1_approve_review` / `v1_request_changes`
- or report that a decision is pending

Never assume approval is always human-only. Never assume approval is always autonomous.

**Accepted-plan follow-ups use append, not a new ideation, while the plan is still open.**
When the user asks for a small one-off addition after `v1_accept_plan_and_schedule`, call
`v1_append_task_to_plan` if the plan branch is still active. Open PR / waiting-on-PR plans are still
open and can receive appended tasks. Once the PR/plan is closed, merged, terminal, or actively
merging/repairing, start a new ideation continuation instead.

**Conservative intervention.** When uncertain, annotate and wait. A wrong auto-action (retrying a
non-transient failure, sending a message to a generating agent) causes more damage than a brief
delay. Escalate to human attention when in doubt.

**Escalations are exception states.** `review:escalated`, `merge_conflict`, and
`merge_incomplete` should be surfaced immediately. Only resolve `review:escalated` if your current
policy/delegation explicitly allows it.

**Report for chat surfaces, not operator consoles.** Telegram, Slack, and WhatsApp updates should
be short:
- line 1: what changed
- line 2: next automatic step or blocker
- line 3 only if user action is truly required
- in shared/multi-project webhook topics, start line 1 with the project label
- stay silent for no-op duplicate redeliveries; users should not see "No response requested" or duplicate-event chatter
- avoid markdown tables in webhook updates; use prose or at most 2-4 compact bullets

Do not ask "shall I approve?" unless approval is genuinely blocked on the user.

---

## 3. Do's and Don'ts

| Situation | ✅ Do | ❌ Don't |
|-----------|-------|---------|
| Task in `review_passed` | Check current authority/policy. If allowed: `v1_approve_review` or `v1_request_changes`. If not: report pending decision. | Assume a human must always approve, or assume you may always auto-approve |
| Task in `escalated` | Annotate + alert human/owner; call `v1_resolve_escalation` ONLY if delegation/policy explicitly allows | Auto-resolve escalation by default |
| `review:escalated` event arrives | `v1_get_task_detail` → `v1_create_task_note` → surface blocker clearly | Treat it like an ordinary `review_passed` case |
| Task in `merge_conflict` | Annotate with conflict files + alert human | Call `v1_retry_task` (resets branch context) |
| Task in `blocked` | Call `v1_get_task_detail` to inspect blocker; notify human if human-input block | Cancel the blocked task |
| MCP/webhook delivery failure | Observe. If RalphX emits `system:webhook_unhealthy`, treat it as informational; system health tracker handles recovery automatically | Synthesize your own webhook-health events or claim transport repair — infrastructure issues, not agent-resolvable |
| `v1_accept_plan_and_schedule` fails | Call `v1_resume_scheduling(session_id)` to resume idempotently | Re-call `v1_accept_plan_and_schedule` (may double-create tasks) |
| Agent `agent_status: generating` | Wait briefly, then check status again | Send a message (it will be queued and may confuse agent state) |
| Rate limit 429 received | Exponential backoff: 1s → 2s → 4s → 8s with ±200ms jitter | Retry immediately or in a tight loop |
| Task in transient state (`pending_review`, `approved`, `pending_merge`) | Check status again shortly for settled state | Take any action — these states last milliseconds |
| Task in `paused` | Annotate + alert human; do NOT call `v1_retry_task` | Call `v1_retry_task` (resets to `ready`, loses pre-pause context) |
| Task iterating `reviewing → re_executing` | Log the cycle count; only intervene after ~5 cycles | Interrupt the loop — normal iteration is 2-3 cycles |
| Freshness conflict (`executing → merging`) | Observe; this is automatic rebasing, not a failure | Panic or annotate — it resolves to `ready` or `pending_review` automatically |
| QA fails (`qa_failed`) | Call `v1_get_task_detail` + surface to human; human decides: `v1_request_changes` or manually skips QA via app UI | Auto-retry QA without human review |
| `merge:ready` or `pending_merge` | Treat as merge-pipeline progress; monitor for `merge:completed` or `merge:conflict` | Ask the user for a second generic merge approval |
| Session `delivery_status = delivered` | Report "all tasks merged to main" / "plan delivered" | Continue talking as if approval is still pending |
| Small follow-up after accepted plan while plan is still open | `v1_append_task_to_plan(session_id, title, steps, acceptance_criteria)` | Start a whole new ideation for a one-off task while the existing plan branch/PR is still open |
| Follow-up after plan is closed, terminal, or actively merging/repairing | Start a new ideation continuation | Append to a closed, merged, terminal, conflicting, or actively merging plan |

---

## 4. Quick Decision Guide

The 11 most common decision points:

**1. Event arrives: `task:status_changed` → `escalated`**
→ `v1_get_task_detail(task_id)` → `v1_create_task_note(task_id, "Escalated: <reason>")` → Alert human

**2. Event arrives: `review_passed`**
→ Check current authority/policy
→ If allowed: inspect `v1_get_review_summary` + `v1_get_task_diff` → call `v1_approve_review` or `v1_request_changes`
→ If not allowed: report decision pending with task title and key finding summary

**3. Event arrives: `merge:conflict`**
→ `v1_get_task_detail(task_id)` for conflict files → `v1_create_task_note(task_id, "Merge conflict in: <files>")` → Alert human

**4. `v1_get_attention_items` returns failed tasks**
→ `v1_get_task_detail(task_id)` for failure reason → Transient failure + count < 3 → `v1_retry_task`
→ Non-transient or count ≥ 3 → Annotate + alert human

**5. `v1_get_ideation_status` shows `agent_status: idle` unexpectedly**
→ `v1_send_ideation_message(session_id, "Please continue drafting the plan and finalize proposals.")`

**6. Plan verification not converging after 10 minutes**
→ `v1_get_plan(session_id)` for gap details → Alert human with gaps list → Await human guidance

**7. `v1_get_execution_capacity` returns `available: 0`**
→ Wait for `task:status_changed` events indicating terminal states (`merged`/`failed`/`cancelled`/`stopped`) — capacity recalculates automatically
→ Do NOT call `v1_resume_scheduling` (not a capacity tool)

**8. Task stuck in `blocked` for extended period**
→ `v1_get_task_detail` → check `blocked_by` list → `v1_batch_task_status([blocker_ids])`
→ Blocker `failed`/`stopped` → annotate + consider `v1_retry_task` on blocker; blocker active → wait

**9. Ideation plan ready — should I accept?**
→ Only if: plan exists AND `v1_get_plan_verification` returns `Verified` status
→ Then: `v1_accept_plan_and_schedule(session_id)` → note returned task IDs

**10. Session delivery reaches `delivered`**
→ Report concise completion: "All tasks merged to main. Plan delivered."
→ Include task count or key titles only if useful

**11. User asks for a small follow-up after plan acceptance**
→ `v1_get_session_tasks(session_id)` to inspect delivery/task state
→ If the plan is still open, including open PR / waiting-on-PR: `v1_append_task_to_plan(session_id, title, steps, acceptance_criteria)`
→ If the PR/plan is closed, merged, terminal, or actively merging/repairing: start a new ideation continuation

---

## 5. Reference Navigation

Five reference files live alongside this skill. Load them when you need depth on a specific topic.
All paths are relative to this skill file's directory (`skills/ralphx-swe/`):

| File | Contents | When to Load |
|------|----------|--------------|
| `reference/state-machine.md` | All 24 pipeline states, transition table, happy paths, behavioral patterns | Confused by a state transition; need precise state semantics |
| `reference/decision-trees.md` | 6 ASCII decision trees for common scenarios | Handling escalation, merge conflict, blocked task, failed task, capacity |
| `reference/event-catalog.md` | All 20 event types, fields, and recommended reactions | Unsure how to react to a specific event; need event field names |
| `reference/failure-playbooks.md` | 5 step-by-step recovery procedures with real `v1_` tool calls | Recovering from accept failure, blocked task, rate limits, idle agent, webhook delivery failure |
| `reference/cross-project.md` | Cross-project orchestration — what it is and what it means for your agent | Understanding tasks appearing across multiple projects from a single ideation session |

These files are fully self-contained. No external dependencies. No internet required.

---

## 6. Section Dispatch

When this skill is invoked with an argument, load the corresponding content:

If `$ARGUMENTS` contains `state-machine`: Read the file `reference/state-machine.md` in the same
directory as this skill file and present its full content to the user.

If `$ARGUMENTS` contains `decisions`: Read the file `reference/decision-trees.md` in the same
directory as this skill file and present its full content to the user.

If `$ARGUMENTS` contains `events`: Read the file `reference/event-catalog.md` in the same
directory as this skill file and present its full content to the user.

If `$ARGUMENTS` contains `recovery`: Read the file `reference/failure-playbooks.md` in the same
directory as this skill file and present its full content to the user.

If `$ARGUMENTS` contains `dos-donts`: Present Section 3 (Do's and Don'ts) of this skill file in
full, with the complete table of situations and recommended actions.

If `$ARGUMENTS` contains `quick-start`: Present Sections 1 (Bootstrap) and 4 (Quick Decision
Guide) of this skill file in full, as a fast onboarding reference.

If `$ARGUMENTS` contains `cross-project`: Read the file `reference/cross-project.md` in the same
directory as this skill file and present its full content to the user.

If `$ARGUMENTS` is empty or not recognized: Present the full skill content above (Sections 1–5),
then remind the user that `/ralphx-shared-plugin:ralphx-swe [section]` can be used to load a
specific reference file.
