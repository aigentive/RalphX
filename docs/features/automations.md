# Automations

Automations run a project-scoped goal as a sequence of ordinary agent conversations. Each run is a normal agent workspace run that can publish a PR, and the automation scheduler advances only from durable automation rows and run rows.

## V1 Contract

- Source of truth: `automations` and `automation_runs`; setup conversations are authoring surfaces only.
- Completion signal: `pr_merged` for `edit` mode runs.
- Run mode: `edit` is the only finalizable mode for `pr_merged` v1 automations. `plan` and `ideation` remain schema-reserved until a non-PR completion signal is designed.
- Single flight: one open automation run at a time, enforced by the run-state machine and repository constraints.
- Conversations: automation-owned setup and run conversations keep project context and are hidden from the normal Agents list. Direct automation links can still open them for audit.
- Visibility: the Automations page is on by default after P7, with runtime flag overrides still available through `ui.feature_flags.automations_page` and `RALPHX_UI_AUTOMATIONS_PAGE`.

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
- Non-PR completion signals such as CI green, human approval, or plan artifact produced.
- Parallel runs within one automation.
- Team-mode automation runs.
- Cross-project automations.
- External/webhook triggers.
- Finalizable `plan` and `ideation` automation modes.

## Validation Checklist

- Signal authority remains attempt-scoped to the run's captured PR number and conversation.
- Status and judge-state writes go through `AutomationTransitionService`.
- Successor creation checks max runs, consecutive failures, and latest-run identity before inserting.
- Stacked chaining creates a `local_branch` successor from the previous PR head and never carries source-PR linkage.
- Automation-owned conversations are excluded from Agents list endpoints but remain directly addressable.
