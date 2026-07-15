> **Maintainer note:** Keep this file compact. Prefer one-line rules, links to source docs, and explicit non-negotiables over prose.

# Native Agent Delegation Bridge

## Goal
- Let any RalphX agent delegate to any canonical RalphX agent on any supported harness through RalphX-owned MCP tools, not harness-native agent discovery.

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
  - async job lifecycle
  - canonical agent lookup from `agents/`
  - explicit harness selection
  - delegated-session creation/linking
  - result/error snapshots
  - cancellation
- Provider runtimes do not own specialized delegation semantics.

## Session Model
- Parent agent calls `delegate_start` with:
  - parent context identity
  - canonical `agent_name`
  - prompt/instructions for the specialist
  - optional harness/model/effort/policy overrides
- Final target model: RalphX creates or reuses a dedicated delegated session for the specialist.
- Current bridge remains ideation-only until that backing model lands.
- The delegated process runs with:
  - `RALPHX_CONTEXT_TYPE=delegation`
  - `RALPHX_CONTEXT_ID=<delegation_session_id>`
  - `RALPHX_PROJECT_ID=<project_id>`
  - canonical `RALPHX_AGENT_TYPE`
- The delegated agent uses normal RalphX MCP tools against that delegated session.

## Continuity Rules
- Phase 1 continuity is RalphX-session continuity, not provider-session continuity.
- Final target: delegation jobs return a delegated-session id so later rounds can reuse the same delegated runtime context.
- Provider-specific resume/session reuse can be added later behind the same MCP contract.

## Native vs Provider Delegation
- Keep provider-native delegation only for generic low-specialization exploration.
- Use RalphX native delegation for any specialized named agent:
  - ideation specialists
  - optional general-purpose plan-review lenses selected by the active model
  - future execution / review / QA specialists

## Phase Plan
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
  - ideation parent -> child session continuity for delegated specialists as a temporary bridge
  - explicit parent turn/message lineage in request metadata, agent env, prompt context, and returned job snapshots
  - per-job status history (`running`, `completed`, `failed`, `cancelled`) on the snapshot contract
  - `delegate_wait` hydration of live child-session status, with optional recent-message inclusion through the existing child-session status contract
- Still required:
  - dedicated delegated-session backing model
  - non-ideation parent contexts
  - provider-session continuity / resume
  - richer live progress relay beyond child-session snapshot hydration
  - prompt migration for specialist paths still assuming Claude-native delegation

## Non-Negotiables
- Canonical `agents/` remains the agent source of truth.
- MCP allowlists remain per-agent and must stay aligned across prompts, `ralphx.yaml`, and MCP server tool exposure.
- Cross-harness specialized delegation must use the RalphX bridge, not provider-specific plugin/subagent discovery hacks.
- Do not widen the bridge beyond ideation on top of `IdeationSession`; use the dedicated delegated-session model instead.
