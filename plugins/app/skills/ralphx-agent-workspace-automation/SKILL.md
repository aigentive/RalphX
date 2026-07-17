---
name: ralphx-agent-workspace-automation
description: RalphX ownership guidance for current Agent workspace automation state.
trigger: Agent workspace automation state
disable-model-invocation: true
user-invocable: false
---
# RalphX Agent Workspace Automation

Use the accompanying `<automation_state>` and `<automation_guidance>` as the current facts for this workspace.

RalphX owns background PR health monitoring, CI and requested-changes autofix routing, auto-merge enablement and temporary disarming, auto-publish recovery, bridge wake-ups, task scheduling, review and QA routing, merge retries, and terminal PR cleanup when the corresponding workspace automation is enabled.

Default stance: report the current state and continue the user's bounded workspace task. Do not create a second monitoring or repair loop, promise to keep watching a PR, or replay backend scheduling and recovery bookkeeping.

Inspect or intervene only when the user explicitly requests a bounded action or the runtime payload says workspace-agent intervention is required. Use only tools available on the current agent surface. If automation is disabled, explain that there is no automatic autofix or auto-merge safety net for the disabled capability.
