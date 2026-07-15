---
name: ralphx-agent-workflow-orchestrator
description: Author safe RalphX Agent Workflow scripts that orchestrate durable delegated agents through the high-level workflow API.
trigger: RalphX Agent Workflow authoring
disable-model-invocation: true
user-invocable: false
---
# RalphX Agent Workflow Orchestrator

Use this skill only in a conversation whose selected capability is Workflow.

Generate a deterministic JavaScript program for the bundled RalphX workflow runner. The program may use only the provided globals: `args`, `meta`, `phase`, `log`, `agent`, `parallel`, `pipeline`, and checkpoint helpers. It has no filesystem, shell, network, environment, process, import, or module access.

## Authoring contract

1. Break the request into named phases and call `phase(name)` before each phase.
2. Give every `agent()` call a stable `logicalKey`. A key must identify the same prompt and output schema across retries.
3. Keep prompts self-contained. Do not interpolate untrusted text into executable JavaScript; pass user values through `args`.
4. Use explicit output schemas for machine-consumed results and reject missing or null required results.
5. Keep fanout within the declared `maxConcurrency` and `maxInvocations` metadata. Prefer the smallest useful fanout.
6. Use `parallel([{ prompt, logicalKey, agentName, schema, title }, ...])` only for independent agent work; this descriptor form is the backend-enforced concurrent fanout path. Use `pipeline()` when later work depends on earlier output.
7. End with a concise structured result that can be reported truthfully to the user.

Call `create_agent_workflow_script` with the script, metadata, permission summary, and estimated fanout. This stores the script for user review; it never approves or starts it. Do not claim that a workflow is running until `start_agent_workflow_run` succeeds after the user-approved hash is present.

Use the run inspection and pause/resume/cancel tools for lifecycle management. Never expose or invent runner attempts, leases, transport frames, delegated job IDs, or recovery bookkeeping.
