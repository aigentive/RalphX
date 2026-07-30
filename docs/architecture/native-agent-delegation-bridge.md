> **Maintainer note:** Keep this file compact. Prefer one-line rules, links to source docs, and explicit non-negotiables over prose.

# Native Agent Delegation Bridge

## Goal
- Let any RalphX agent delegate to any canonical RalphX agent on any supported harness through RalphX-owned MCP tools, not harness-native agent discovery.
- Standing named teammates, routed messages, and Team lifecycle build on this bridge per `rx-native-team-mode.md`; they are not properties of an ordinary delegation job.

## Why
- Claude `Task(...)` and Codex native subagents are not a stable cross-harness contract.
- Codex custom-agent discovery is fixed-location and conflicts with user-managed `.codex/agents`.
- Specialized RalphX agents need canonical prompts, MCP allowlists, session linking, and auditability independent of provider-native agent mechanics.

## Source Pattern
- Reefbot coordination mode is the reference pattern:
  - provider-facing MCP tools stay stable
  - backend owns async delegation jobs, cancellation, continuity, and progress snapshots
  - provider runtimes only receive tool surface + coordination metadata

## Contract
- MCP tools:
  - `delegate_start`
  - `delegate_wait`
  - `delegate_cancel`
- Backend owns:
  - reusable delegated launch/reuse orchestration through `NativeDelegationLauncher`
  - HTTP one-shot job lifecycle, snapshots, cancellation, and lifecycle projection
  - canonical agent lookup from `agents/`
  - explicit harness selection
  - delegated-session creation/linking
  - result/error snapshots
  - cancellation
- Provider runtimes do not own specialized delegation semantics.

## Session Model
- Parent agent calls `delegate_start` with:
  - canonical `agent_name`
  - prompt/instructions for the specialist
  - optional caller task reference
  - optional harness/model/effort/policy overrides
- Caller/parent identity is injected from trusted runtime transport.
- RalphX creates or reuses a dedicated delegated session for the specialist.
- The delegated process runs with:
  - `RALPHX_CONTEXT_TYPE=delegation`
  - `RALPHX_CONTEXT_ID=<delegation_session_id>`
  - `RALPHX_PROJECT_ID=<project_id>`
  - canonical `RALPHX_AGENT_TYPE`
- The delegated agent uses normal RalphX MCP tools against that delegated session.

## Continuity Rules
- Fresh child fields resolve independently: explicit `delegate_start` value → compatible effective Delegated Subagent role/provider value → harness fallback.
- A parent conversation or delegated-session harness is lineage, not a fresh-child default.
- Provider-derived model/effort values remain inherited runtime attribution; they are not relabeled as explicit caller choices.
- Reusing a delegated-session id pins its stored specialist identity and harness; conflicting agent or explicit harness requests fail before status, conversation, process, or job mutation.
- Compatible reuse preserves existing provider-session continuation precedence behind the same MCP contract.

## Native vs Provider Delegation
- Keep provider-native delegation only for generic low-specialization exploration.
- Use RalphX native delegation for any specialized named agent:
  - ideation specialists
  - optional general-purpose plan-review lenses selected by the active model
  - future execution / review / QA specialists

## Historical Phase Plan
- Phase 1:
  - ideation-family sessions only
  - backend-owned `delegate_start/wait/cancel`
  - direct canonical agent spawn
  - temporary ideation child-session creation + result snapshots
- Phase 2:
  - dedicated delegated-session backing model
  - `ChatContextType::Delegation`
  - bridge migration off ideation child sessions
- Phase 3:
  - broader context support beyond ideation parents
  - persistent continuity / provider resume
  - prompt migration from Claude-only specialist assumptions
- Phase 4:
  - execution/review/QA specialist adoption
  - richer progress events / relay

## Current State
- Landed:
  - HTTP endpoints and MCP tool exposure for `delegate_start`, `delegate_wait`, `delegate_cancel`
  - backend delegation job registry with running/completed/failed/cancelled snapshots
  - canonical agent lookup + harness-aware spawn through the existing runtime clients
  - dedicated `DelegatedSession` entity/repositories and `ChatContextType::Delegation`
  - delegated conversations with provider-session continuation and explicit session reuse
  - ideation, project, task-like, and nested-delegation parents; standalone remains unsupported
  - explicit parent turn/message lineage in request metadata, agent env, prompt context, and returned job snapshots
  - exact caller-task reservation, delegated-run settlement, and startup orphan recovery
  - application-layer launch results that remain independent of one-shot HTTP job snapshots
  - per-job status history (`running`, `completed`, `failed`, `cancelled`) on the snapshot contract
  - `delegate_wait` hydration of delegated-session status with optional recent messages
- Still required:
  - prompt migration for specialist paths still assuming Claude-native delegation
  - standing roster/member/message semantics described in `rx-native-team-mode.md`

## Non-Negotiables
- Canonical `agents/` remains the agent source of truth.
- MCP allowlists remain per-agent and must stay aligned across prompts, `ralphx.yaml`, and MCP server tool exposure.
- Cross-harness specialized delegation must use the RalphX bridge, not provider-specific plugin/subagent discovery hacks.
- All supported parent contexts use the dedicated delegated-session model; never reintroduce `IdeationSession` as generic delegation storage.
