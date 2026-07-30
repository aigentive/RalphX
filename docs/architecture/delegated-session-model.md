> **Maintainer note:** Keep this file compact. Prefer one-line rules, links to source docs, and explicit non-negotiables over prose.

# Delegated Session Model

## Problem
- `IdeationSession` is not a valid backing model for general delegated specialists.
- Delegated work must not inherit ideation-specific semantics:
  - proposals
  - plan ownership / inherited plan state
  - verification generation / rounds / confirmation gates
  - ideation-only UI and orchestration assumptions

## Decision
- Add a dedicated delegated-session entity before widening the native delegation bridge beyond ideation parents.
- Keep the current bridge ideation-only until that entity exists.

## Goal
- Let any parent context delegate to a specialist without coupling runtime continuity to ideation state.

## Required Entity
- `delegation_sessions` or `agent_sessions` as a first-class backend record.

## Minimum Fields
- `id`
- `project_id`
- `parent_context_type`
- `parent_context_id`
- `parent_turn_id`
- `parent_message_id`
- `agent_name`
- `harness`
- `status`
- `provider_session_id`
- `created_at`
- `updated_at`
- `completed_at`
- optional `title`
- optional `error`

## Required Context Contract
- Add a dedicated chat/runtime context such as `ChatContextType::Delegation`.
- Delegated specialists should run with:
  - `RALPHX_CONTEXT_TYPE=delegation`
  - `RALPHX_CONTEXT_ID=<delegation_session_id>`
  - `RALPHX_PROJECT_ID=<project_id>`
  - canonical `RALPHX_AGENT_TYPE`
- Parent lineage should stay additive:
  - `RALPHX_PARENT_CONTEXT_TYPE`
  - `RALPHX_PARENT_CONTEXT_ID`
  - optional `RALPHX_PARENT_TURN_ID`
  - optional `RALPHX_PARENT_MESSAGE_ID`

## Runtime Expectations
- `delegate_start` creates or reuses a delegated session, not an ideation session.
- `delegate_wait` hydrates delegated-session runtime status/messages, not ideation child-session status.
- `delegate_cancel` stops the delegated runtime and marks the delegated session terminal.

## Continuity
- RalphX continuity key is the delegated session id.
- Provider continuity is additive through `provider_session_id`, not by reusing ideation session ids.
- `NativeDelegationLauncher` launches/reuses this durable session and returns the delegated
  session/conversation/run outcome; HTTP `DelegationJobSnapshot` state is a separate one-shot
  wrapper concern.

## Phase 0 Implementation Cut
- add delegated-session entity + repository
- add DB migration
- add `ChatContextType::Delegation`
- add working-directory/project-resolution support for delegation context
- switch native delegation bridge storage/runtime env from ideation child sessions to delegated sessions
- keep MCP contract stable where possible

## Current State
- Landed:
  - delegated-session entity + id type
  - delegated-session repository trait
  - memory + SQLite delegated-session repositories
  - SQLite migration creating `delegated_sessions`
  - `ChatContextType::Delegation`
  - delegated-session AppState wiring
  - conversation/runtime helpers for delegation context
  - native bridge migration off ideation child-session storage
  - delegated conversation/provider-session continuation and explicit session reuse
  - exact caller-task assignment settlement and startup orphan recovery
- Separate concern:
  - standing roster/member/message lifecycle belongs to `rx-native-team-mode.md`

## Non-Negotiables
- Do not reintroduce `IdeationSession` as generic cross-context delegation storage.
- Do not couple delegated specialist history to ideation proposal/verification state.
- Canonical `agents/` remains the source of truth for delegated specialist identity and prompt content.
