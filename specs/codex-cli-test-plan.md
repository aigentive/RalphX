# Codex CLI Test Plan

Status: planning phase. This document defines the regression and validation strategy for adding Codex CLI without regressing existing Claude behavior.

## 1. Testing principles

- replay raw provider events wherever possible
- test shared orchestration once against normalized events, not once per provider UI branch
- keep Claude green throughout every phase
- prefer narrow targeted runs over broad rebuild churn

## 2. Test categories

### 2.1 Unit tests

Need unit coverage for:

- harness capability detection
- spawn arg/config translation
- logical policy to provider-policy mapping
- normalized event mapping
- provider error classification
- lane settings resolution

### 2.2 Repository and migration tests

Need coverage for:

- new provider-neutral conversation/session columns
- backward compatibility with old `claude_session_id` rows
- lane settings persistence
- additive migrations for harness/model/effort metadata

### 2.3 Replay parser tests

Required fixtures:

- Claude raw stream fixtures from current RalphX behavior
- Codex raw JSONL fixtures derived from generic samples

Assertions:

- equivalent normalized assistant text events
- equivalent normalized tool lifecycle events
- equivalent normalized subagent lifecycle events
- usage extraction
- completion/failure mapping

### 2.4 Orchestration tests

Need coverage for:

- queue/pause/stop/resume semantics per harness
- startup recovery per harness
- stale session handling per harness
- reconciliation classification per harness
- ideation verification child-session flows on Codex

### 2.5 Frontend tests

Need coverage for:

- provider-neutral type parsing
- settings transforms for per-lane harness/model/effort
- chat widgets rendering normalized events only
- subagent cards with Codex-backed child runs
- diagnostics/availability warnings

## 3. High-signal existing suites to extend

Backend:

- `src-tauri/src/infrastructure/agents/claude/mod_tests.rs`
- `src-tauri/src/infrastructure/agents/claude/claude_code_client_tests.rs`
- `src-tauri/src/infrastructure/agents/claude/agent_config/tests.rs`
- `src-tauri/src/application/chat_resumption_tests.rs`
- `src-tauri/src/application/chat_service/chat_service_handlers_tests.rs`
- `src-tauri/src/infrastructure/sqlite/sqlite_chat_conversation_repo_tests.rs`

Frontend:

- `frontend/src/types/agent-profile.test.ts`
- `frontend/src/api/chat.test.ts`
- `frontend/src/hooks/useIdeationModelSettings.test.ts`

## 4. New test surfaces to add

Backend:

- Codex capability detection tests
- Codex parser replay tests
- normalized-event contract tests shared across Claude and Codex fixtures
- provider-neutral conversation repository tests
- lane settings repository/command tests

Frontend:

- harness-aware settings hook tests
- provider-neutral chat conversation schema tests
- normalized event widget tests with Codex-shaped scenarios

## 5. Milestone-by-milestone validation

### Milestone 1. Harness abstraction

Run:

- targeted Rust unit tests for shared orchestration and Claude harness wrapper

Must prove:

- Claude behavior unchanged

### Milestone 2. Compatibility migrations

Run:

- migration tests
- repository tests
- frontend schema parsing tests

Must prove:

- old data still reads
- new fields persist

### Milestone 3. Normalized event pipeline

Run:

- replay fixtures for Claude
- chat UI transform tests

Must prove:

- downstream consumers no longer depend on Claude raw payload shape

### Milestone 4. Codex capability + parser

Run:

- Codex capability detection tests
- Codex replay parser tests

Must prove:

- Codex raw logs normalize correctly before user-facing routing lands

### Milestone 5. Codex ideation + verification

Run:

- ideation runtime/handler tests
- queue/recovery tests
- frontend settings and widget tests

Must prove:

- Codex ideation and verification flows work while Claude still works

## 6. Manual validation checklist

Before calling phase-1 complete:

- run ideation on Claude
- run ideation on Codex
- run verification on Claude
- run verification on Codex
- confirm Codex subagent/delegation events appear in UI
- confirm raw logs and prompt captures exist for both harnesses
- confirm pause/stop/resume behavior remains correct
- confirm startup recovery does not mis-handle Codex sessions

## 7. Test data and fixtures policy

- keep small committed fixtures for normalized parser tests
- keep representative large/raw examples under docs or test fixtures, not hidden in ad hoc local paths
- every provider-specific fixture should document which real sample it came from and what behaviors it is meant to lock
