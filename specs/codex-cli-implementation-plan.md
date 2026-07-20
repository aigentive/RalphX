# Codex CLI Implementation Plan

Status: planning phase. This document turns the discovery spec into the concrete execution order for implementing Codex CLI support in RalphX.

Parent references:

- `specs/codex-cli.md`
- `specs/codex-cli-contract.md`
- `specs/codex-cli-backlog.md`
- `docs/ai-docs/reefagent-codex-cli.md`
- `docs/ai-docs/reefagent-codex-responses.md`

## 1. Delivery strategy

Implementation should land in narrow vertical slices with compatibility shims first and naming cleanup later.

Non-negotiable sequencing:

1. introduce abstractions before renames
2. introduce normalized parsing before UI rewiring
3. land Codex ideation + verification first
4. defer execution/review/merge Codex rollout until recovery semantics are proven

## 2. Phase plan

### Phase 0. Contract freeze

Goal:

- finalize the provider-neutral contracts before runtime edits

Deliverables:

- finalized harness enum
- finalized normalized event schema
- finalized lane settings schema
- finalized provider-session compatibility strategy

Acceptance criteria:

- no unresolved naming ambiguity around `claude_session_id`, `claudeCode`, or Claude-only model aliases

### Phase 1. Harness abstraction substrate

Goal:

- separate shared orchestration from provider-specific spawn/parsing

Core work:

- introduce provider-neutral harness types:
  - `AgentHarnessKind`
  - `HarnessCapabilities`
  - `HarnessSpawnRequest`
  - `HarnessResumeRequest`
  - `HarnessRunContext`
  - `RawHarnessEvent`
  - `NormalizedHarnessEvent`
- extract Claude-specific spawn building and parser logic behind a harness boundary
- keep the existing Claude behavior as the first implementation of the abstraction

Likely files:

- `src-tauri/src/application/chat_service/**`
- `src-tauri/src/infrastructure/agents/claude/**`
- `src-tauri/crates/ralphx-domain/src/agents/**`

Acceptance criteria:

- Claude remains functionally unchanged
- shared orchestration no longer imports Claude parsing/spawn details directly

### Phase 2. Compatibility migrations and provider-neutral persistence

Goal:

- add schema and API compatibility layers required before Codex sessions exist

Core work:

- add provider-neutral conversation session fields
- add provider-neutral run metadata fields
- persist harness kind on agent runs and ideation session effective runs
- preserve backward compatibility for existing data and UI callers during migration

Likely files:

- `src-tauri/src/infrastructure/sqlite/migrations/**`
- `src-tauri/src/infrastructure/sqlite/sqlite_chat_conversation_repo.rs`
- `src-tauri/src/infrastructure/sqlite/sqlite_agent_run_repo.rs`
- `src-tauri/crates/ralphx-domain/src/entities/chat_conversation.rs`
- `src-tauri/crates/ralphx-domain/src/repositories/chat_conversation_repository.rs`
- `src-tauri/src/commands/unified_chat_commands.rs`
- frontend chat conversation and response types

Acceptance criteria:

- old rows still load
- new provider-neutral fields exist
- Claude continues to resume correctly through the compatibility layer

### Phase 3. Normalized event pipeline

Goal:

- make downstream consumers provider-neutral before Codex parser lands

Core work:

- add normalized event mapper for Claude stream events
- store raw provider events unchanged
- adapt chat-service/UI consumers to normalized events
- keep any provider-specific rendering only where it is truly vendor-specific and justified

Likely files:

- `src-tauri/src/application/chat_service/chat_service_types.rs`
- `src-tauri/src/application/chat_service/chat_service_streaming.rs`
- `src-tauri/src/application/chat_service/chat_service_send_background.rs`
- `src-tauri/src/application/team_stream_processor.rs`
- frontend chat widgets and activity-stream consumers

Acceptance criteria:

- Claude paths render from normalized events
- replay tests exist for current Claude raw events

### Phase 4. Codex harness availability and capability detection

Goal:

- detect whether the local Codex binary supports the required feature surface

Core work:

- implement Codex binary discovery
- implement version/help-surface capability detection
- define minimum supported capability set
- surface unavailable/degraded Codex status in settings and diagnostics

Likely files:

- new `src-tauri/src/infrastructure/agents/codex/**`
- settings commands
- frontend settings/diagnostics surfaces

Acceptance criteria:

- RalphX can explain why Codex is unavailable or degraded
- no Codex spawn happens if required capabilities are missing

### Phase 5. Codex raw parsing and logging

Goal:

- ingest Codex events without yet routing all user flows to Codex

Core work:

- implement Codex raw event logger
- implement Codex JSONL envelope parser
- map nested item/tool/subagent/usage events into the normalized contract
- add replay fixtures from Reefagent samples

Likely files:

- new Codex parser/logging modules
- shared raw event persistence
- parser replay tests

Acceptance criteria:

- captured Codex sample events normalize successfully
- child/delegation events and usage fields are preserved

### Phase 6. Codex ideation and verification rollout

Goal:

- land the first functional milestone requested by the user

Core work:

- add per-lane harness selection for:
  - ideation
  - ideation verification
  - ideation subagents
  - verification subagents
- translate MCP/tool grants to Codex run config
- route prompt construction into Codex spawn transport
- operate Codex in explicit solo mode

Likely files:

- `ralphx.yaml`
- ideation settings repos/commands
- `src-tauri/src/application/chat_service/chat_service_context.rs`
- ideation runtime handlers
- settings UI

Acceptance criteria:

- ideation can run on Codex
- verification can run on Codex
- Codex-native subagents work in those flows
- Claude can still run the same flows when selected

### Phase 7. Recovery, reconciliation, and queue semantics

Goal:

- make Codex-backed ideation durable under the same orchestration rules

Core work:

- resume/retry classification per harness
- stale session recovery behavior per harness
- pause/stop/queued-message handling for Codex sessions
- startup recovery and reconciliation awareness of harness kind

Likely files:

- `src-tauri/src/application/chat_resumption.rs`
- `src-tauri/src/application/startup_jobs.rs`
- `src-tauri/src/application/reconciliation/**`
- `src-tauri/src/commands/execution_commands/**`

Acceptance criteria:

- Codex ideation sessions survive restarts and reconciliation checks correctly
- provider errors do not get misclassified through Claude-only rules

### Phase 8. Execution-lane Codex expansion

Goal:

- optional later milestone, only after phases 1-7 are stable

Core work:

- task execution harness routing
- reviewer/merger harness routing
- RX-native Team capability routing through provider-neutral delegation

Acceptance criteria:

- only pursued once phase-1 Codex lanes are stable in real use

## 3. Workstreams that can run in parallel

After phase 0 is frozen:

- backend abstraction and compatibility migrations
- frontend provider-neutral schema/UI work
- replay fixture collection and parser test scaffolding

After phase 3 is stable:

- Codex capability detection
- Codex parser/logging
- lane settings persistence and UI

## 4. Commit strategy

Commit every major milestone.

Recommended commit boundaries:

1. discovery/spec checkpoint
2. implementation plan + migration/test plan docs
3. harness abstraction substrate
4. compatibility migrations
5. normalized Claude event pipeline
6. Codex capability detection + parser
7. Codex ideation/verification routing
8. recovery/reconciliation follow-through

## 5. Historical phase-1 exclusions

Do not include in the first implementation milestone:

- vendor-specific Team parity (superseded by RX-native Team)
- Codex worker/reviewer/merger rollout
- removal of legacy Claude compatibility fields before all consumers are migrated
- aggressive prompt-source refactor beyond what is needed to generate Codex-compatible runtime config

## 6. Ready-to-start coding gate

Coding should start only after these docs are accepted:

- `specs/codex-cli.md`
- `specs/codex-cli-contract.md`
- `specs/codex-cli-implementation-plan.md`
- `specs/codex-cli-migrations.md`
- `specs/codex-cli-test-plan.md`
