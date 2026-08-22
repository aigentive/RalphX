---
paths:
  - "src-tauri/src/infrastructure/agents/claude/**"
  - "src-tauri/src/infrastructure/agents/codex/**"
  - "src-tauri/src/application/chat_service/**"
  - "src-tauri/src/infrastructure/sqlite/migrations/*thinking*"
  - "frontend/src/hooks/useChatEvents*"
  - "frontend/src/hooks/useChatRecovery*"
  - "frontend/src/components/Chat/**"
  - "docs/architecture/agent-thinking-capture.md"
  - "docs/architecture/claude-spawning-system.md"
  - "docs/architecture/agent-harnesses.md"
  - "CLAUDE.md"
  - "AGENTS.md"
---

> **Maintainer note:** Keep this file compact. Put explanations, protocol examples, debugging, and test maps in `docs/architecture/agent-thinking-capture.md`.

# Agent Thinking Capture

**Required context:** `multi-harness.md` | `event-coverage-checklist.md` | `stateful-workflow-review.md` | `docs/architecture/agent-thinking-capture.md`

## Non-Negotiables

| Rule | Contract |
|---|---|
| Provider-exposed content only | “Thinking” means summaries/deltas explicitly emitted by the harness CLI. Never claim RalphX captures hidden chain-of-thought. |
| Harness adapters normalize | Claude and Codex keep their native parsers; normalize into shared `ContentBlockItem::Thinking` + `agent:thinking`, not pairwise frontend branches. |
| Capability probes fail closed | Optional CLI flags are enabled only by falsifiable capability evidence. Claude help text owns `--include-partial-messages`; `--thinking-display` uses help marker OR the value-rejection acceptance probe (bogus value must be rejected with non-zero exit + stderr echoing both flag and probe value); ❌ unknown-arg + `--version` exit status, ❌ unknown-arg + `--help` exit status (both exit 0). |
| One spawn-arg seam | `spawn_args::shared_streaming_cli_args` is the sole production source of `--output-format stream-json` / `--verbose` / `--include-partial-messages` / `--thinking-display summarized`; both the chat/base builder and `ClaudeCodeClient::build_cli_args` must consume it. ❌ Re-adding these flags inline in any builder. |
| Backend owns lifecycle | The backend emits authoritative logical `block_index`, `is_settled`, `duration_ms`, and provider-reported `reasoning_tokens`; the UI must not infer them from array position, tools, or run completion. |
| One wire contract | `AgentThinkingPayload` is the event authority. Any field change must keep Rust serialization fixtures and frontend fixture consumers aligned. |
| Settle never erases | Claude settle uses empty text + append semantics; frontend merging must preserve accumulated text and ignore an unmatched empty settle. |
| Empty blocks never surface | Reject empty/whitespace thinking at persistence and persisted/live render barriers; migrations are hygiene, not the compatibility boundary. |
| One frontend owner | Event-driven thinking mutation stays in `useChatEvents`; `useChatRecovery` may hydrate the same transcript from durable/cache state; `ChatMessageList` owns manual thinking-group intent and defaults every unrecorded group to expanded. ❌ Parallel stores, settlement writers, or expansion effects. |
| Preserve legacy hydration | Persisted blocks without `isSettled` remain finalized by default; provider-neutral changes stay additive for historical Claude data. |
| Test the seam | Prove native capture → normalized event → serialized payload → frontend merge/render. Parser-only or hand-written frontend payload tests are insufficient. |
| Keep docs synchronized | Harness flag, native event-shape, payload, persistence, or UI lifecycle changes update the canonical architecture document in the same change. |

## Ownership

| Concern | Owner |
|---|---|
| Claude CLI capability/args | `cli_capabilities.rs` + `claude_code_client.rs` |
| Claude stream lifecycle | `claude/stream_processor/` |
| Codex reasoning normalization | `codex/stream_processor.rs` |
| Shared event/persistence | `chat_service_types.rs` + `chat_service_streaming.rs` |
| Live frontend merge/recovery | `frontend/src/hooks/useChatEvents.ts` + `frontend/src/hooks/useChatRecovery.ts` |
| Live/persisted presentation | `ChatMessageList.liveRows.ts` + `MessageItem.tsx` |
| Canonical explanation/test map | `docs/architecture/agent-thinking-capture.md` |

## Change Gate

1. Identify the harness-native event and capture a representative fixture when vendor shape changed.
2. Extend the existing harness parser and shared payload; do not add a new UI-only schema.
3. Prove stale-run rejection, ordering, settlement, absence of phantom/empty blocks, and persistence parity.
4. For a new harness, document whether output is delta or complete, whether duration/token progress exists, and how capability support is established.
5. Run only the focused tests listed in the canonical architecture document.
