# Automations

Automations run a project-scoped goal as a sequence of ordinary agent conversations. Each run is a normal agent workspace run that can publish a PR, and the automation scheduler advances only from durable automation rows and run rows.

## V1 Contract

- Source of truth: `automations` and `automation_runs`; setup conversations are authoring surfaces only.
- Completion signals: `pr_merged` for `edit` mode runs; `ideation_finalized` for the task-graph bridge.
- Run modes: `edit` publishes one PR per run. `ideation` is a single-run bridge that turns its verified plan into proposals, task dependencies, and World A execution. `plan` remains schema-reserved.
- Single flight: one open automation run at a time, enforced by the run-state machine and repository constraints.
- Conversations: automation-owned setup and run conversations keep project context and are hidden from the normal project/publication Agents lists. They surface in the sidebar only when grouped by Automation; direct automation links can still open them for audit.
- Visibility: the Automations page is on by default after P7, with runtime flag overrides still available through `ui.feature_flags.automations_page` and `RALPHX_UI_AUTOMATIONS_PAGE`.

## Run View Contract

Automation run presentation is state-only on the wire and selector-owned in the frontend. Detail pages, compact automation panels, and focused run conversations all use the same run view rules for open state, cancellability, stage copy, PR copy, composer locking, and phase chips.

- Run detail responses include `plan_phase`, `plan_artifact_id`, `plan_approved_by`, `plan_approved_artifact_version`, and `plan_approved_at`. Approval-match and plan-phase fields are scoped to the open run; `plan_artifact_id` remains available for any run linked to a Planning session so terminal run plans stay auditable.
- Run conversations open through the setup conversation with an `automation_run` focus overlay. Opening a run never selects a separate sidebar row, and terminalization does not downgrade the visible run audit view.
- Run conversation tabs are policy-driven: Automation is always present, Plan is visible but disabled until the run has a plan artifact, PR appears only when the run has PR metadata, and Commit & Publish appears only when the focused run has a publishable agent workspace. Integration tabs remain hidden for run surfaces.
- Parked `awaiting_plan_approval` runs keep the composer editable for plan revisions. Other runs are read-only while the latest run still holds goal authority; fully settled audit conversations may accept chat turns without re-entering the automation lifecycle.
- Run update events carry the owning automation id and refresh only that automation detail. Automation-level events refresh lists, the detail record, and the Automation sidebar grouping.

## Outline Authoring

Starting an automation from the Automations page uses trusted one-shot authoring. The setup agent turns the outline into the complete goal, ordered goal-item list, first-run prompt, spec, and automatic plan/merge policy, then calls the dedicated decomposition verifier. The verifier checks coverage, phase boundaries, ordering, and autonomy risk against the exact persisted inputs. Approval atomically records that input snapshot before activation; any concurrent edit makes the verdict stale and leaves the draft inactive. A revision verdict returns actionable findings to the setup agent instead of finalizing.

Direct automation setup conversations retain the reviewed mode: the setup agent may draft and revise the configuration, but activation remains an explicit user decision. Legacy rows without authoring metadata are treated as reviewed and unverified.

Setup conversations expose the versioned setup Plan and the same Commit & Publish surface as ordinary agent workspaces, plus caller-bound lifecycle, judge recovery, status, and publish tools. The server derives the automation from the injected setup-conversation identity; these tools do not accept an automation id, run id, or publish conversation id from the model. Publish actions reuse the ordinary agent-workspace readiness, update-from-base, repair, push, and draft-PR pipeline. The trusted publish target is the setup workspace when present, otherwise the latest run workspace; no eligible bound workspace fails closed. Run conversations keep their own versioned implementation Plan for audit while publication remains owned by the shared automation workspace surface.

## Lifecycle And Recovery

- **Pause** is resumable and leaves the current durable run state intact. Resume accepts only `Paused`; it cannot reactivate a stopped automation.
- **Stop / Cancel automation** is terminal for the current run attempt. It cancels every open run, clears parked plan-judge state, disarms applicable automatic PR merge, and preserves completed/cancelled history for audit.
- **Restart automation** is the explicit recovery path from `Stopped`. It moves the automation back to `Active` with compare-and-swap protection and creates a new pending run from the last durable run prompt/base. It never mutates a cancelled run back to pending. If fresh-run creation fails, the status is compensated back to `Stopped` while the original failure is returned.
- **Cancel run** affects only the current latest run selected by the server. A later Run Now or automation restart creates a separate run row.
- **Retry judge** redispatches only the latest signal-terminal run whose terminal judge is `Failed`. **Retry plan judge** requires the latest parked run, the exact current parked artifact, and a `Failed` plan judge; stale artifacts and superseded attempts are rejected.

## Run Plan Gate

Every run plans before it implements. The run conversation is provisioned in workspace **plan mode**: the read-only plan profile explores the repo and authors a versioned plan artifact in a hidden Planning ideation session, then the run parks at `awaiting_plan_approval`. Approval switches the same conversation to edit mode and delivers the implement prompt; from there the run behaves exactly as before (publish, merge, run-level judge).

Parking is driven by the current agent turn's durable `Completed` state, not provider-process exit. This lets a Claude interactive process remain alive after ending the planning turn while the scheduler advances immediately. A plan artifact by itself is never completion evidence: the agent run must have started after the automation phase began and must be completed, so stale artifacts and earlier turns cannot park the run.

Per-automation config (editable at any status via settings; also exposed through MCP `update_automation`):

| Field | Values | Default |
|---|---|---|
| `plan_approval_mode` | `manual` \| `automatic` | `manual` |
| `pr_merge_mode` | `manual` \| `automatic` | `manual` |
| `plan_deep_verification` | off \| on | off |

- **Manual approval:** open the parked run's conversation (runs-list pill deep-links to it), review the plan in the artifact pane, click Approve. Chatting with the parked run requests revisions — the turn re-enters `running` and re-parks with a new plan version. Stale approvals never deliver: the gate matches the approval's artifact id against the session's current plan artifact id.
- **Automatic approval:** the plan judge (harness-keyed `plan_judge_model`; sonnet-class on Claude, gpt-5.4 on Codex) verdicts the plan against the goal and current goal item. Approve writes the same native approval (`approved_by = judge`); revise sends instructions back to the run agent, bounded by `plan_max_revision_rounds` (default 3 judge-issued revisions) and a repeat-instruction fingerprint guard. Judge failures always fall back to a paused automation awaiting human review — never auto-approve, never run failure. A human approval on a plan-gate-paused automation auto-resumes it.
- **Deep verification (optional):** each new plan version additionally runs the native ideation plan-verification loop; the judge holds until it terminates and receives the outcome as advisory context. Verification failures degrade to "verification unavailable" and never block the gate.
- **Automatic PR merge:** when `pr_merge_mode` is `automatic`, publication arms GitHub native auto-merge (squash) on the run PR; cancel disarms it. Enable failures surface as a persistent action-required notification plus a live run warning and degrade to manual merge. Rejected for `pr_head_stacked` chains — auto-merging an earlier PR in a stacked chain would retarget successor bases mid-flight.

The terminal judge may propose a complete replacement goal-item list when implementation discovers that unfinished work should be added, split, or reordered. Completed/skipped history is immutable. The proposal is attached to the successor run's planning context but does not mutate the active goal immediately: the existing successor plan approval is the authorization boundary. Approval applies the proposal with an exact-snapshot compare-and-swap before edit mode starts. A human goal edit while paused rejects the pending proposal; a concurrent/stale edit pauses with `goal_replan_stale` and never overwrites newer work.

## Ideation Task-Graph Bridge

An automation configured with `run_mode = ideation`, `completion_signal = ideation_finalized`, and `plan_deep_verification = true` uses its normal hidden Planning session as the handoff into the task pipeline. Approval is accepted only when the current plan artifact has reached native `Verified` status. The scheduler then changes the owning workspace to ideation mode and delivers one `<auto-propose>` turn that creates scoped proposals, records dependency edges, and finalizes them through the normal proposal-to-task entry path.

The generic human acceptance gate is bypassed only when authoritative state proves all of the following: the session is verified, the approved artifact is current, the automation and latest run are active, the run owns the linked workspace conversation, and the run uses the ideation bridge contract. Any stale or missing link fails closed. The bridge run completes only after the session is `Accepted`; an agent that exits before acceptance is a failed run, and restart recovery redelivers only the approved current plan.

Once accepted, World A owns task scheduling, dependency unblocking, review, and local merge recovery. Automation detail responses expose this lineage as one pipeline read model (session, plan, proposal count, task statuses, and blockers), so the detail screen can show progress after the automation run itself has completed.

PR-status polling retries one transient infrastructure failure immediately, then falls back to the durable consecutive-failure counter and pauses at the configured threshold. Every system pause reason appears in the live attention center and durable notification history. Paused-automation notifications expose a direct Resume action; the backend still requires the automation to be Paused, so stale notification actions fail closed and refresh attention state.

Migration note: existing Active automations default to `manual` and park at their next run's plan gate — the runs-list pill and paused-reason banner are the discovery path. Runs provisioned before the upgrade never enter plan-phase code paths.

## Chaining

`merged_base` is the default chain mode. After a terminal run and a `continue` judge verdict, the next run bases on the automation base. If run 1 started from a source PR, run 2 drops `source_pull_request` and uses run 1's recorded PR base branch as `local_branch`.

`pr_head_stacked` bases each continued run on the previous run's PR head branch. Judge verdicts for stacked automations must choose `previous_pr_head`; `automation_base` is rejected for stacked mode, and a missing previous PR head is a validation error. This prevents silent fallback to the wrong base and keeps stacked successors in isolated branch mode without source-PR linkage.

## Usage Totals

Automation detail responses include aggregate usage totals from the agent runs attached to each automation run conversation:

- input tokens
- output tokens
- cache creation tokens
- cache read tokens
- estimated USD cost when recorded

The detail UI displays these totals in the configuration summary. Per-run drilldown remains the linked conversation, which continues to expose the normal provider/model/runtime usage surfaces.

## Draft Hygiene

Drafts are validated fail-closed before activation. Required fields include goal, provider/model, run mode, completion signal, resolved base, guardrail values, and the first run prompt. Incomplete drafts remain visible in the Automations list for cleanup; v1 does not auto-delete idle drafts.

## Deferred Beyond V1

- Cron or time-based triggers.
- Additional completion signals such as CI green or human approval.
- Parallel runs within one automation.
- Team-mode automation runs.
- Cross-project automations.
- External/webhook triggers.
- Finalizable standalone `plan` automation mode.

## Validation Checklist

- Signal authority remains attempt-scoped to the run's captured PR number and conversation.
- Status and judge-state writes go through `AutomationTransitionService`.
- Terminal agent-run evidence is freshness-scoped to `agent_phase_started_at`; parked time never counts toward `max_run_duration`.
- Plan approvals are written only through the actor-parameterized approval helper and read only via the artifact-id match — never bare approval status.
- Gate prompts flow through exactly one `send_message` with spawn proof (`was_queued == false`); queued landmines are purged; `run_prompt` stays byte-identical to the judge's `next_run_prompt`.
- User-initiated mode switches on automation-run conversations are rejected at the command layer; only the scheduler's system path switches `plan -> edit` or `plan -> ideation`.
- Automation delete archives each run's plan artifact chain and deletes its approval row and Planning session.
- Successor creation checks max runs, consecutive failures, and latest-run identity before inserting.
- Stacked chaining creates a `local_branch` successor from the previous PR head and never carries source-PR linkage.
- Automation-owned conversations are excluded from Agents list endpoints but remain directly addressable.
