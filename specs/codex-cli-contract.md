# Codex CLI Phase-1 Contract

Status: approved planning target for the first implementation wave. This document converts the discovery work into exact contract decisions for the first coding phase.

Parent references:

- `specs/codex-cli.md`
- `specs/codex-cli-implementation-plan.md`
- `specs/codex-cli-migrations.md`
- `specs/codex-cli-test-plan.md`

## 1. Scope of this contract

This contract covers the exact shapes RalphX should code against for phase 1:

- provider-neutral session and run metadata
- provider-neutral lane settings
- provider-neutral normalized event payloads
- Codex ideation + verification + subagent support
- compatibility behavior for existing Claude-shaped fields

This contract does not force execution/review/merge Codex rollout in phase 1.

## 2. Core decisions

### 2.1 Harness enum

Use a first-class harness enum everywhere new work is added:

- `claude`
- `codex`

Rust recommendation:

- `AgentHarnessKind::Claude`
- `AgentHarnessKind::Codex`

Serialization:

- snake_case strings in persisted/API surfaces

### 2.2 Provider session storage

Do not reuse `claude_session_id` as the canonical cross-provider field.

Canonical new fields:

- `provider_session_id: Option<String>`
- `provider_harness: Option<AgentHarnessKind>`

Compatibility rule:

- keep `claude_session_id` during the transition
- for Claude runs only:
  - write `provider_session_id`
  - write `provider_harness = claude`
  - mirror to `claude_session_id`
- for Codex runs:
  - write `provider_session_id`
  - write `provider_harness = codex`
  - never write `claude_session_id`

### 2.3 Provider-neutral run metadata

`agent_runs` must become the source of truth for runtime metadata that is currently inferred from volatile stream/UI state.

Additive run fields for phase 1:

- `harness`
- `provider_session_id`
- `logical_model`
- `effective_model_id`
- `logical_effort`
- `effective_effort`
- `approval_policy`
- `sandbox_mode`

All are additive. No existing `agent_runs` columns are removed in phase 1.

### 2.4 Lane settings storage

Do not keep stretching the current ideation model/effort tables.

Decision:

- add a new additive lane-settings table
- keep `execution_settings` and `global_execution_settings` for concurrency/autocommit semantics only
- keep `ideation_model_settings` and `ideation_effort_settings` as fallback/legacy read sources during migration

Recommended new table:

- `agent_lane_settings`

Columns:

- `id INTEGER PRIMARY KEY`
- `scope_type TEXT NOT NULL`
  - `global`
  - `project`
- `scope_id TEXT NULL`
  - `NULL` for global
  - project id for project rows
- `lane TEXT NOT NULL`
- `harness TEXT NOT NULL`
- `model TEXT NULL`
- `effort TEXT NULL`
- `approval_policy TEXT NULL`
- `sandbox_mode TEXT NULL`
- `fallback_harness TEXT NULL`
- `updated_at TEXT NOT NULL`

Unique index:

- `(scope_type, scope_id, lane)`

### 2.5 Lane enum

Phase-1 lane keys are fixed to:

- `ideation_primary`
- `ideation_verifier`
- `ideation_subagent`
- `ideation_verifier_subagent`
- `execution_worker`
- `execution_reviewer`
- `execution_reexecutor`
- `execution_merger`

Notes:

- phase 1 only actively routes Codex through the first four lanes
- execution lanes may keep using Claude while still being representable in the same settings model

## 3. Settings contract

### 3.1 Lane settings value model

Each lane row stores:

- `harness`
- `model`
- `effort`
- `approval_policy`
- `sandbox_mode`
- `fallback_harness`

Rules:

- `model`, `effort`, `approval_policy`, `sandbox_mode`, `fallback_harness` may be null to mean “inherit/fall through”
- `harness` is always explicit in a stored row

### 3.2 Resolution chain

New lane-settings resolution order:

1. project `agent_lane_settings` row
2. global `agent_lane_settings` row
3. YAML default for the lane/harness
4. hardcoded product default

Legacy fallback during migration only:

- for ideation lanes, if no `agent_lane_settings` row exists yet:
  - fall back to existing `ideation_model_settings`
  - fall back to existing `ideation_effort_settings`
  - fall back to YAML Claude config

### 3.3 Logical effort enum

New logical effort surface:

- `low`
- `medium`
- `high`
- `xhigh`

Compatibility rule:

- old Claude `max` maps to logical `xhigh`

Translation:

- Claude:
  - `low -> low`
  - `medium -> medium`
  - `high -> high`
  - `xhigh -> max`
- Codex:
  - pass through to `model_reasoning_effort`

### 3.4 Default phase-1 lane values

Global defaults:

- `ideation_primary`
  - `harness = codex`
  - `model = gpt-5.4`
  - `effort = xhigh`
  - `approval_policy = on-request`
  - `sandbox_mode = workspace-write`
- `ideation_verifier`
  - `harness = codex`
  - `model = gpt-5.4-mini`
  - `effort = medium`
  - `approval_policy = on-request`
  - `sandbox_mode = workspace-write`
- `ideation_subagent`
  - `harness = codex`
  - `model = gpt-5.4-mini`
  - `effort = medium`
- `ideation_verifier_subagent`
  - `harness = codex`
  - `model = gpt-5.4-mini`
  - `effort = medium`
- execution lanes:
  - remain on Claude defaults initially

Note:

- if local Codex capability detection shows phase-1 requirements missing, Claude remains the active harness regardless of configured default, with an explicit degradation warning

## 4. YAML contract

### 4.1 Keep existing `claude:` block

Do not delete or rename the existing `claude:` runtime block in phase 1.

### 4.2 Add a parallel `codex:` runtime block

Top-level block:

- `codex:`

Owned concerns:

- binary discovery hints
- default model
- default reasoning effort
- approval policy defaults
- sandbox defaults
- MCP transport defaults
- capability overrides if needed for local skew

### 4.3 Add lane defaults block

New top-level block:

- `agent_harness_defaults:`

Shape:

```yaml
agent_harness_defaults:
  ideation_primary:
    harness: codex
    model: gpt-5.4
    effort: xhigh
    approval_policy: on-request
    sandbox_mode: workspace-write
  ideation_verifier:
    harness: codex
    model: gpt-5.4-mini
    effort: medium
  execution_worker:
    harness: claude
    model: sonnet
```

### 4.4 Env overrides

Phase-1 env override pattern:

- `RALPHX_AGENT_HARNESS_<LANE>`
- `RALPHX_AGENT_MODEL_<LANE>`
- `RALPHX_AGENT_EFFORT_<LANE>`
- `RALPHX_AGENT_APPROVAL_POLICY_<LANE>`
- `RALPHX_AGENT_SANDBOX_MODE_<LANE>`

Examples:

- `RALPHX_AGENT_HARNESS_IDEATION_PRIMARY=codex`
- `RALPHX_AGENT_MODEL_IDEATION_VERIFIER=gpt-5.4-mini`

## 5. API contract

### 5.1 New lane settings API

Add new Tauri/API surfaces instead of overloading old ideation model/effort endpoints forever.

New commands:

- `get_agent_lane_settings`
- `update_agent_lane_settings`

Response shape:

```json
{
  "scope": "global",
  "projectId": null,
  "lanes": {
    "ideation_primary": {
      "harness": "codex",
      "model": "gpt-5.4",
      "effort": "xhigh",
      "approvalPolicy": "on-request",
      "sandboxMode": "workspace-write",
      "fallbackHarness": "claude",
      "effective": {
        "harness": "codex",
        "model": "gpt-5.4",
        "effort": "xhigh",
        "approvalPolicy": "on-request",
        "sandboxMode": "workspace-write",
        "source": "global"
      }
    }
  }
}
```

### 5.2 Old ideation model/effort commands

Compatibility decision:

- keep existing ideation model/effort commands in phase 1
- they become legacy UI/backfill surfaces
- once the new lane settings UI is live, stop extending the old endpoints

### 5.3 Chat and run payloads

Add new provider-neutral payload fields:

- `harness`
- `providerSessionId`
- `effectiveModelId`
- `effectiveModelLabel`
- `effectiveEffort`

Compatibility decision:

- keep `claudeSessionId` / `claude_session_id` in phase 1 only where the current frontend expects it
- mark it legacy in code comments and stop adding new consumers

## 6. Normalized event contract

### 6.1 Keep existing event names for phase 1

Do not churn every emitter/listener name immediately.

Keep names such as:

- `agent:run_started`
- `agent:chunk`
- `agent:tool_call`
- `agent:task_started`
- `agent:task_completed`
- `agent:run_completed`
- `agent:error`

Change the payload contract to be provider-neutral.

### 6.2 Exact payload requirements

`agent:run_started`

- `runId`
- `conversationId`
- `contextType`
- `contextId`
- `harness`
- `providerSessionId?`
- `effectiveModelId?`
- `effectiveModelLabel?`
- `effectiveEffort?`
- `runChainId?`
- `parentRunId?`

`agent:tool_call`

- `toolName`
- `toolId?`
- `arguments`
- `result?`
- `error?`
- `conversationId`
- `contextType`
- `contextId`
- `harness`
- `parentToolUseId?`
- `seq`

`agent:task_started`

- `toolUseId`
- `toolName`
- `description?`
- `subagentType?`
- `model?`
- `teammateName?`
- `harness`
- `conversationId`
- `contextType`
- `contextId`
- `seq`

`agent:task_completed`

- `toolUseId`
- `agentId?`
- `totalDurationMs?`
- `totalTokens?`
- `totalToolUseCount?`
- `teammateName?`
- `harness`
- `conversationId`
- `contextType`
- `contextId`
- `seq`

`agent:run_completed`

- `conversationId`
- `contextType`
- `contextId`
- `harness`
- `providerSessionId?`
- `effectiveModelId?`
- `effectiveEffort?`
- `runChainId?`

`agent:error`

- `conversationId?`
- `contextType`
- `contextId`
- `harness`
- `error`
- `stderr?`
- `providerErrorCategory?`
- `providerSessionId?`

### 6.3 Raw event persistence

Persist raw events per harness without translation loss.

Required raw metadata:

- `run_id`
- `conversation_id`
- `harness`
- `provider_session_id`
- `raw_kind`
- `raw_payload`
- `created_at`
- `parse_error?`

## 7. Harness-specific behavior contract

### 7.1 Claude

Claude remains:

- default execution harness in phase 1
- supported through the same RX-native Team contract as other capable harnesses

### 7.2 Codex

Codex constraints:

- subagents flow through provider-neutral delegation support
- no Claude plugin-dir assumption

### 7.3 MCP transport

Contract decision:

- Claude keeps plugin-dir and current MCP path
- Codex uses generated per-run config overrides with explicit MCP server definitions
- RalphX core owns one logical tool/MCP grant graph; harness adapters render vendor-specific allowlists

## 8. Frontend contract

### 8.1 New UI source of truth

The eventual settings UI should read from lane settings, not separate ideation model and effort hooks.

New frontend concepts:

- `HarnessKind`
- `LaneKey`
- `LaneSettings`
- `EffectiveLaneSettings`

### 8.2 Legacy compatibility

Keep old hooks temporarily:

- `useIdeationModelSettings`
- `useIdeationEffortSettings`

But they should not be extended to support Codex-specific semantics beyond the migration window.

### 8.3 Chat rendering

Chat widgets and task subagent cards must render from:

- normalized event payloads
- harness/model metadata in those payloads

They must not parse Claude raw structures directly.

## 9. Phase-1 compatibility window

Compatibility fields kept temporarily:

- `claude_session_id`
- `claudeSessionId`
- `claudeCode`
- old ideation model/effort commands
- Claude-only model alias settings UI

Compatibility window end condition:

- new lane settings UI is live
- chat consumers use provider-neutral fields
- Claude and Codex replay tests pass against normalized events

Only then should removal of legacy Claude-shaped contracts be planned.

## 10. Explicit non-decisions for later phases

These are intentionally deferred:

- whether agent profiles become a fully versioned v2 API
- whether old ideation model/effort tables are eventually deleted
- whether execution lanes default to Codex later
- whether event names themselves are renamed beyond payload normalization

## 11. Ready-for-coding checklist

Phase-1 coding can start once implementation follows these exact contract choices:

- additive provider-neutral conversation/run metadata
- additive `agent_lane_settings` table
- new lane settings API
- normalized event payloads on existing event names
- Codex solo-mode only for ideation/verifier lanes
