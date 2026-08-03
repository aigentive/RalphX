# Agent Thinking Capture

This document is the canonical reference for how RalphX captures, normalizes, persists, and renders provider-exposed thinking/reasoning across supported agent harnesses.

## Scope and terminology

RalphX can display only reasoning text that a harness CLI explicitly emits through its supported stream protocol. This is usually a provider-generated summary or a stream of exposed thinking deltas. It is not hidden model chain-of-thought, and product copy, logs, tests, and documentation must not imply otherwise.

| Term | Meaning in RalphX |
|---|---|
| Thinking/reasoning | Provider-exposed text normalized into a `ContentBlockItem::Thinking` block. |
| Streaming block | A Claude block receiving `thinking_delta` events and not yet sealed. |
| Settled block | A complete provider block. Claude settles on `content_block_stop`; Codex reasoning items arrive complete. |
| Logical block index | Position the block occupies in RalphX `content_blocks`; this is the backend authority used by events, persistence, and UI merging. |
| Duration | Wall-clock time between Claude thinking block start/stop when both are observed. Codex currently exposes no equivalent. |
| Token progress | Claude CLI `thinking_tokens` progress. This is an estimate attached to the currently unsettled live block, not persisted reasoning usage. Codex exposes only a final aggregate `reasoning_output_tokens` count. |

## End-to-end flow

```text
harness CLI JSONL
  -> harness-native parser
  -> provider-exposed reasoning text
  -> ContentBlockItem::Thinking
  -> chat service persistence + agent:thinking event
  -> useChatEvents (conversation + run scoped)
  -> live thinking rows / ThinkingGroupToggle
  -> finalized timeline + persisted MessageItem hydration
```

The harness parsers remain native because Claude and Codex do not expose the same protocol. The shared contract begins at `ContentBlockItem::Thinking` and the `agent:thinking` event; frontend consumers do not branch on harness.

## Harness capture matrix

| | Claude CLI | Codex CLI |
|---|---|---|
| Process mode | `--output-format stream-json --verbose` | `codex exec --json` |
| Requested reasoning presentation | `--thinking-display summarized` only when help advertises it | `model_reasoning_summary="concise"` config override |
| Partial delivery | `--include-partial-messages` only when help advertises it | Complete reasoning items in observed exec JSONL |
| Native input | `content_block_start` → `thinking_delta`* → `content_block_stop`; verbose `assistant.content[type=thinking]` fallback | Observed `item.completed` with `item.type == "reasoning"` and flat `text`; rollout `agent_reasoning`/`summary` compatibility |
| RalphX event cadence | One unsettled event per non-empty delta, then one settle event | One settled event per extracted complete reasoning item |
| Duration | Measured locally when start and stop are both present | `None` |
| Token progress | Optional `system.subtype == "thinking_tokens"` → `agent:thinking_progress` | `turn.completed.usage.reasoning_output_tokens`, attached to the final complete reasoning block; no per-summary split is inferred. |
| Native fixture | Claude processor sidecar fixtures/tests | `codex/fixtures/exec_json_reasoning_0_146_0.jsonl` |

### Claude

Every Claude spawn path requests stream JSON through one shared seam: `spawn_args::shared_streaming_cli_args` is consumed by both `ClaudeCodeClient::build_cli_args` (agentic client spawns) and `build_base_cli_command_inner_with_runtime_context_and_profile` (chat-service launch plans). Optional flags are gated through cached `ClaudeCliCapabilities`:

- `--include-partial-messages` enables native delta delivery when supported.
- `--thinking-display summarized` asks the CLI for its summarized thinking presentation when supported.
- If either flag is unavailable or probing fails, RalphX omits it. Claude 4.x CLIs may still expose complete thinking text in verbose assistant messages; the Claude 5 family (Fable/Opus 5) withholds thinking text entirely (`"thinking": ""` plus signature) unless `--thinking-display summarized` is passed, so omitting that flag leaves only `thinking_tokens` progress.

`StreamProcessor` owns the block lifecycle:

1. `content_block_start(type=thinking)` flushes pending text, starts the thinking buffer, and records an `Instant`.
2. Each non-empty `thinking_delta` appends to the buffer and emits `StreamEvent::Thinking { text, block_index }`.
3. `content_block_stop` seals one `ContentBlockItem::Thinking`, records duration, and emits one `ThinkingSettled` with the same index/duration.
4. A verbose `AssistantContent::Thinking` is already complete, so the processor emits `Thinking` followed immediately by `ThinkingSettled { duration_ms: None }`.
5. A verbose summary is suppressed after native deltas so one provider block is not duplicated.

The event index is the future/current RalphX `content_blocks` position computed before insertion. Consumers must not recompute it from mutable processor state or assume it is the provider envelope's raw `index`.

### Codex

Both Codex exec argument builders set `model_reasoning_summary="concise"`. `parse_codex_event_line` and `extract_codex_reasoning` normalize the provider shapes:

- Live captured shape: `item.completed` + `item.type == "reasoning"` + flat `text`.
- The declared exec schema also permits `item.started` / `item.updated`; RalphX accepts non-empty reasoning text on `item.*`.
- Rollout compatibility: `agent_reasoning` completed items and `summary[]` text.
- `agent_reasoning_delta` is intentionally not accepted because it is not a real event in the captured Codex exec schema.

Each extracted item is complete. `process_codex_stream_background` appends and persists one thinking block, then emits `agent:thinking` with `append_to_previous: false`, `is_settled: true`, and no duration. A real `codex-cli 0.146.0` capture also exposes `turn.completed.usage.reasoning_output_tokens`; RalphX updates only the final reasoning block from that turn and emits one empty-text settled update with the aggregate. The CLI provides no per-item allocation or duration, so neither is inferred.

## Capability probing

Optional flags must be detected with a falsifiable signal.

Claude's real CLI may exit successfully when `--version` appears anywhere, even beside an unknown flag. Therefore this is invalid:

```text
claude --unknown-optional-flag --version -> exit 0 -> “supported”  ❌
```

Unknown flags combined with `--help` also exit zero on 2.1.220, so `--help` exit status is equally invalid as acceptance evidence.

The supported flow is:

```text
resolve production CLI path
  -> claude --version
  -> claude --help
  -> parse help for exact optional flag
  -> if --thinking-display absent from help: value-rejection acceptance probe
  -> cache by resolved CLI path
  -> include only proven flags
```

`parse_claude_cli_capabilities` uses exact help markers for `--include-partial-messages` and `--thinking-display`. Claude CLI 2.1.220 removed `--thinking-display` from `--help` while still accepting it, so when the help marker is absent `probe_claude_cli` runs a value-rejection acceptance probe: `claude --thinking-display <bogus-value> --help`. The flag counts as supported only when the command exits non-zero AND stderr contains both the `option '--thinking-display` marker and the echoed bogus probe value — a CLI that rejects unknown options mentions the flag but never echoes the value, so that shape stays unsupported. `--include-partial-messages` is a boolean flag with no value to reject, so it remains help-marker-only. Spawn failures during the probe fail closed. A version floor is acceptable only when established from vendor evidence and representative versions; unknown support must omit the optional flag rather than risk breaking every spawn.

Capability tests must model the real short-circuit behavior: an unknown argument combined with `--version` or `--help` can exit zero while help still omits the flag.

## Shared event contract

`AgentThinkingPayload` is serialized as `agent:thinking`:

| Field | Contract |
|---|---|
| `text` | Delta/complete text. A settle-only event uses `""`. |
| `run_id` | Current agent-run identity when available; frontend rejects a different active run. |
| `block_index` | Authoritative RalphX logical block position. |
| `conversation_id`, `context_type`, `context_id` | Routing scope. |
| `seq` | Monotonic event order within the stream emitter. |
| `append_to_previous` | Claude deltas/settle: `true`; complete Codex item: `false`. |
| `duration_ms` | Present only when measured/known. |
| `reasoning_tokens` | Provider-reported aggregate reasoning output tokens, present only on Codex's final settled update; it belongs to the final reasoning block and is not a per-summary count. |
| `is_settled` | Always emitted by current producers; `false` for Claude deltas, `true` for settle/complete items. |

Claude settlement deliberately sends empty text with append semantics. `useChatEvents` also preserves existing text defensively. Changing settlement to replacement semantics would erase the accumulated reasoning.

The exact serialized streaming and settled shapes live in:

- `src-tauri/tests/fixtures/agent_thinking_payload.streaming.json`
- `src-tauri/tests/fixtures/agent_thinking_payload.settled.json`
- `src-tauri/tests/fixtures/agent_thinking_payload.codex_settled.json`

Rust asserts these fixtures match `AgentThinkingPayload`; frontend tests import both committed settled fixtures. Any wire change must update both sides in the same change.

## Persistence and hydration

Thinking is represented in two durable projections plus one transient cache:

| Surface | Contents and authority |
|---|---|
| `chat_messages.content_blocks` | Ordered assistant JSON, including `Thinking { text, duration_ms, reasoning_tokens }`; used by transcript/modal hydration paths. |
| `chat_message_blocks` | Canonical paged timeline row with `kind='thinking'`, original logical `block_index`, text, and optional `metadata.duration_ms` / `metadata.reasoning_tokens`. |
| `StreamingStateCache.partial_thinking_segments` | Transient partial text only. It is not settlement/duration authority and is cleared with the stream lifecycle. |
| `StreamingStateCache.tool_calls` | Transient tool snapshots with an optional logical `block_index`; recovery inserts indexed tools between indexed text/thinking anchors and appends legacy unindexed snapshots. |

`persist_timeline_snapshot` skips empty/whitespace thinking while retaining original logical indices. Finalized snapshots delete obsolete rows by exact retained-index membership, so gaps are valid.

Migration `20260731111346_purge_empty_thinking_blocks` removes legacy empty/ASCII-whitespace timeline thinking rows and their payload rows. It does not rewrite `chat_messages.content_blocks`; persisted render guards are the compatibility boundary for that JSON. The migration is irreversible, so production rollout should back up the database first.

## Frontend lifecycle

`useChatEvents` is the only event-driven thinking-state writer:

1. Reject events for another conversation/context or a mismatched active `run_id`.
2. Match the authoritative `block_index`; when token progress created a synthetic block, adopt the latest unsettled synthetic block.
3. Append Claude deltas without clearing accumulated text.
4. Apply settle metadata to the existing block; an unmatched empty settle is a no-op, not a phantom block.
5. Attach token progress to the latest unsettled block. If all known blocks are settled, create a new synthetic running block.

`useChatRecovery` is the complementary hydration path. It reconciles durable timeline rows and the transient streaming cache into the same live transcript after reload or event loss; it does not reinterpret settlement or replace the event contract.

Presentation ownership:

- `buildLiveTranscriptRows` hides settled-and-empty blocks, keeps running/token-only blocks visible, and coalesces visibly adjacent thinking blocks into `thinking_group` rows. Its tool-activity scan stops at thinking blocks so they cannot be consumed by a tool run.
- `synchronizeThinkingGroupExpansion` is the sole automatic expansion writer: the latest running group expands, settled/older groups collapse, and explicit user intent wins. Group keys stay anchored to the first segment as later segments append.
- Live rows treat missing `isSettled` as running for backward compatibility.
- `MessageItem` drops empty persisted blocks, groups visibly adjacent thinking segments, and treats historical blocks without `isSettled` as settled. Hidden tool blocks do not split a group; visible transcript elements do.
- `ThinkingGroupToggle` owns aggregate labels such as “Agent thinking…”, token progress, and “Agent thought for … · N steps”; expanded groups reuse one bounded `ThinkingWidget` body.

## Failure and recovery edges

| Edge | Expected behavior |
|---|---|
| Claude help omits optional flag | For `--thinking-display`, run the value-rejection acceptance probe before omitting; otherwise omit it and spawn remains usable with whatever native thinking shape the CLI exposes. |
| Capability probe fails | Fail closed for optional flags; never guess support from an unrelated successful command. |
| Claude has no partial-message support | Verbose complete thinking emits one `Thinking` + one immediate settle, without duration. |
| Claude 5 family without `--thinking-display` | Thinking text is withheld (`"thinking": ""` + signature); only `thinking_tokens` progress renders, as a token-count pill with no text. |
| Empty provider thinking | No durable/live settled pill; non-thinking content remains. |
| Settle arrives without its delta/block | Frontend ignores the empty settle. |
| Stale run emits late thinking | Frontend rejects it by active `run_id`. |
| Claude process stops before `content_block_stop` | No duration/settle exists to emit. Live UI may remain “thinking” until terminal persisted re-render; this is an accepted limitation. |
| Persisted legacy block lacks lifecycle fields | Hydrate as settled; optional duration stays absent. |
| Codex reasoning has no duration | Render settled without inventing a duration; attach `reasoning_output_tokens` only when authoritative turn-complete usage provides it. |

Do not synthesize provider duration from run duration, tool timing, or frontend timestamps. Those measure different things.

## Adding or changing a harness

1. Capture a representative native JSONL fixture from the actual CLI/version; document provenance without assuming vendor-internal event names.
2. Extend the owning harness parser and normalize only provider-exposed non-empty text.
3. Decide whether each native item is delta or complete and whether duration/token progress can be proved.
4. Emit the shared payload with backend-owned index/lifecycle; keep provider differences out of frontend state.
5. Persist through `ContentBlockItem::Thinking` and both existing write projections; preserve logical ordering and empty guards.
6. Add parser tests, production chat-service entry coverage, payload-fixture parity, frontend merge/render tests, and visual states.
7. Update this document, `.claude/rules/agent-thinking-capture.md`, and user-visible harness limitations in the same change.

## Debugging map

| Symptom | First checks |
|---|---|
| Claude spawn rejects a thinking flag | Resolved CLI path; `probe_claude_cli`; actual `--help`; cached capabilities; final `build_cli_args`. |
| Claude pill never settles | Native stop event; `ThinkingSettled`; emitted payload `is_settled`; active `run_id`; `useChatEvents` merge. |
| Reasoning is duplicated | Claude delta + verbose dedup guard; Codex repeated item lifecycle events; `append_to_previous`. |
| Wrong pill receives text/progress | Backend logical `block_index`; synthetic block adoption; latest-unsettled progress selection. |
| Empty pills reappear | Provider parser emptiness, `persist_timeline_snapshot`, `MessageItem`, and `buildLiveTranscriptRows`. |
| Rehydrated label differs from live | Timeline metadata/JSON casing transform; live missing-field defaults vs persisted finalized defaults. |

## Focused proof locations

| Obligation | Tests/fixtures |
|---|---|
| Claude capability falsification and CLI args | `cli_capabilities_tests.rs`, `claude_code_client_tests.rs` |
| Claude delta/seal/verbose lifecycle | `claude/stream_processor/tests/orchestration_tests.rs` |
| Codex captured event shapes | `codex/stream_processor.rs` tests + `codex/fixtures/exec_json_reasoning_0_146_0.jsonl` |
| Service event sequence, persistence guard | `chat_service_streaming_tests.rs` |
| Rust wire shape | `chat_service_types_tests.rs` + `tests/fixtures/agent_thinking_payload.*.json` |
| Frontend merge, stale-run, progress targeting | `frontend/src/hooks/useChatEvents.test.ts` |
| Empty live/persisted rendering | `ChatMessageList.liveRows.test.ts`, `MessageItem.test.tsx` |
| Grouping and collapse ownership | `thinking-group.test.ts`, `ChatMessageList.liveRows.test.ts`, `ChatMessageList.thinkingLifecycle.test.tsx`, `MessageItem.test.tsx` |
| UI states | `frontend/tests/visual/views/chat/chat-widget-matrix.spec.ts` |

Use the narrow validation commands routed by `src-tauri/CLAUDE.md`, `frontend/src/CLAUDE.md`, and `.claude/rules/rust-test-execution.md`; do not broaden to full suites for documentation-adjacent changes.
