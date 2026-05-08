# Codex CLI Handoff

Updated: 2026-04-07

## Worktree

- Repo: `/Users/example/ralphx-worktrees/codex`
- Branch: `codex-cli-discovery`
- Discovery PR: `https://github.com/aigentive/ralphx.app/pull/17`

## Files already created in this pass

- `AGENTS.md`
- `specs/codex-cli.md`
- `specs/codex-cli-contract.md`
- `specs/codex-cli-implementation-plan.md`
- `specs/codex-cli-migrations.md`
- `specs/codex-cli-test-plan.md`
- `specs/codex-cli-backlog.md`
- `docs/ai-docs/codex-cli/README.md`
- `docs/ai-docs/codex-cli/index.txt`
- `docs/ai-docs/codex-cli/cli-overview.md`
- `docs/ai-docs/codex-cli/cli-reference.md`
- `docs/ai-docs/codex-cli/features.md`
- `docs/ai-docs/codex-cli/slash-commands.md`
- `docs/ai-docs/codex-cli/config-basics.md`
- `docs/ai-docs/codex-cli/config-advanced.md`
- `docs/ai-docs/codex-cli/config-reference.md`
- `docs/ai-docs/codex-cli/config-sample.md`
- `docs/ai-docs/codex-cli/authentication.md`
- `docs/ai-docs/codex-cli/approvals-security.md`
- `docs/ai-docs/codex-cli/sandboxing.md`
- `docs/ai-docs/codex-cli/agents-md.md`
- `docs/ai-docs/codex-cli/rules.md`
- `docs/ai-docs/codex-cli/hooks.md`
- `docs/ai-docs/codex-cli/mcp.md`
- `docs/ai-docs/codex-cli/skills.md`
- `docs/ai-docs/codex-cli/subagents.md`
- `docs/ai-docs/codex-cli/models.md`
- `docs/ai-docs/codex-cli/noninteractive.md`
- `docs/ai-docs/reefagent-codex-cli.md`
- `docs/ai-docs/reefagent-codex-responses.md`

## Official Codex facts already confirmed

- Official docs root exists at `https://developers.openai.com/codex`
- Useful routes already confirmed:
  - `/codex/cli`
  - `/codex/cli/features`
  - `/codex/cli/reference`
  - `/codex/cli/slash-commands`
  - `/codex/config-basic`
  - `/codex/config-advanced`
  - `/codex/config-reference`
  - `/codex/config-sample`
  - `/codex/auth`
  - `/codex/agent-approvals-security`
  - `/codex/concepts/sandboxing`
  - `/codex/guides/agents-md`
  - `/codex/rules`
  - `/codex/hooks`
  - `/codex/mcp`
  - `/codex/skills`
  - `/codex/subagents`
  - `/codex/noninteractive`
  - `/codex/models`
- Official docs confirm:
  - `gpt-5.5` is the recommended starting point for complex reasoning and coding
  - `gpt-5.4` remains a current lower-cost Codex-compatible model
  - `gpt-5.4-mini` is recommended for lighter tasks and subagents
  - `model_reasoning_effort` exists in Codex config
  - `approval_policy` exists in Codex config
  - `mcp_servers.*` exists with `enabled_tools` / `disabled_tools`
  - Codex has native subagents, rules, hooks, skills, MCP, AGENTS.md support, and `codex exec`

## Local Codex binary facts already confirmed

- `codex --version` returned `0.1.2505172129`
- Homebrew cask version reported `0.116.0`
- Installed local help surface is materially smaller than current official docs
- Conclusion already recorded in docs/spec:
  - RalphX needs Codex capability detection and likely a minimum supported Codex version

## Reefagent findings not yet fully merged into the docs

### Core architecture

- Reefagent uses model-prefix routing in `/Users/example/Code/reefbot.ai/src/registry.ts`
  - `pro:` -> `claude-code`
  - `codex:` -> `codex-cli`
- Both CLI providers set `managesOwnSessions = true`
- Shared CLI-agent substrate exists:
  - `src/providers/cli-agent/process-runner.ts`
  - `src/providers/cli-agent/message-serializer.ts`
  - `src/providers/cli-agent/session-manager.ts`
  - `src/providers/cli-agent/availability.ts`
  - `src/providers/cli-agent/inactivity-timer.ts`
- There is still no single unified provider base class; Claude and Codex keep different spawn args and parsers

### Codex vs Claude provider differences in Reefagent

- Claude:
  - uses `--add-dir ORIGINAL_CWD`
  - prompt on `stdin`
  - stdio MCP config file
  - wrapped tool ids like `mcp__reefagent__exec`
  - native parser in `src/providers/claude-code.ts`
- Codex:
  - uses `-C ORIGINAL_CWD`
  - prompt after `--`
  - inline `-c mcp_servers.reefagent.*`
  - logical tool names like `exec` or `jira__get_myself`
  - normalized parser in `src/core/codex-jsonl.ts`
  - config keys include `executionPolicy` and `reasoningEffort`

### Codex MCP bridge in Reefagent

- Claude uses stdio MCP via `src/providers/cli-agent/mcp-config.ts` and `src/commands/mcp.ts`
- Codex uses HTTP JSON-RPC bridge at `/Users/example/Code/reefbot.ai/src/gateway/http.ts` on `/mcp/codex`
- Both converge on shared tool execution in `/Users/example/Code/reefbot.ai/src/mcp/server.ts`

### Reefagent config examples

- `codex.model: codex:gpt-5.5`
- `codex.reasoningEffort: xhigh`
- `codex.executionPolicy: supervised`
- `codex-agent.executionPolicy: autonomous`
- `codex-mini-agent.model: codex:codex-mini`

### Reefagent caveats

- `docs/providers.md` example still shows Codex `enabled_tools` with wrapped MCP names, while code/tests use logical names
- Codex config comments imply tighter sandbox differences than current code actually enforces
- current Codex policies still map to `danger-full-access`; approval policy is the main live difference

## Reefagent raw Codex event findings not yet fully merged into docs

- Raw logs live under `~/.reefagent/sandbox/logs/codex-events/`
- Reefagent wraps Codex events in JSONL envelopes with:
  - `run_started`
  - `stdout_event`
  - `run_finished`
- Observed nested event types:
  - `thread.started`
  - `turn.started`
  - `item.started`
  - `item.completed`
  - `item.updated`
  - `turn.completed`
- Observed nested item types / patterns:
  - `agent_message`
  - `mcp_tool_call`
  - `command_execution`
  - `error`
  - `collab_tool_call` for delegation
- `turn.completed` includes usage:
  - `input_tokens`
  - `cached_input_tokens`
  - `output_tokens`
- Child/delegate sessions use ids like:
  - `parentSession:delegate:codex:1`
- Prompt captures live separately in `~/.reefagent/sandbox/logs/prompts/codex-stream-effective-*.txt`
- Raw event payloads do not directly carry sandbox mode / approval policy; those are visible in config/patch/spawn code instead

## RalphX findings not yet fully merged into the spec

### Current embed points

- `ralphx.yaml` only has a first-class `claude:` runtime block today
- live runtime wiring instantiates Claude clients/services, not Codex, even though `ClientType` in `src-tauri/crates/ralphx-domain/src/agents/types.rs` already contains `Codex`
- major Claude-specific spawn/bootstrap files:
  - `src-tauri/src/infrastructure/agents/claude/mod.rs`
  - `src-tauri/src/infrastructure/agents/claude/claude_code_client.rs`
  - `src-tauri/src/application/chat_service/chat_service_context.rs`
  - `src-tauri/src/http_server/handlers/teams/spawn.rs`
  - `src-tauri/src/http_server/handlers/teams/spawn_execution.rs`
  - `src-tauri/src/application/memory_orchestration.rs`
  - `src-tauri/src/lib.rs`

### Claude-specific persistence/settings

- `chat_conversations.claude_session_id` is the persisted provider session field
- ideation effort persistence is Claude-shaped:
  - `primary_effort`
  - `verifier_effort`
- ideation model persistence is Claude-shaped:
  - `inherit|sonnet|opus|haiku`
  - plus verifier/ideation subagent buckets
- agent profile APIs and frontend types are named `claudeCode`
- agent-profile domain types are Claude-shaped:
  - `src-tauri/crates/ralphx-domain/src/agents/agent_profile.rs`
  - `Model = opus|sonnet|haiku`
  - `ClaudeCodeConfig`
  - `AgentProfile.claude_code`
- chat/event payloads are Claude-shaped:
  - `src-tauri/src/application/chat_service/chat_service_types.rs`
  - `AgentRunCompletedPayload.claude_session_id`
  - `frontend/src/types/chat-conversation.ts`
  - `claudeSessionId` and `claude_session_id`
- ideation model/effort domain types are Claude-shaped:
  - `src-tauri/crates/ralphx-domain/src/ideation/model_settings.rs`
  - `ModelLevel = inherit|sonnet|opus|haiku`
  - `src-tauri/crates/ralphx-domain/src/ideation/effort_settings.rs`
  - `EffortLevel = low|medium|high|max|inherit`

### Recovery / reconciliation / UI areas already confirmed as impacted

- `src-tauri/src/application/startup_jobs.rs`
- `src-tauri/src/application/chat_resumption.rs`
- `src-tauri/src/application/reconciliation/handlers/execution.rs`
- `src-tauri/src/application/reconciliation/handlers/merge.rs`
- `src-tauri/src/commands/execution_commands/settings.rs`
- `src-tauri/src/commands/execution_commands/lifecycle.rs`
- `src-tauri/src/application/task_transition_service.rs`
- `src-tauri/src/application/team_stream_processor.rs`
- `src-tauri/src/application/chat_service/chat_service_streaming.rs`
- `src-tauri/src/application/chat_service/chat_service_errors.rs`
- `frontend/src/types/agent-profile.ts`
- `frontend/src/types/settings.ts`
- `frontend/src/components/settings/IdeationModelSection.tsx`
- `frontend/src/api/chat.ts`
- `frontend/src/types/chat-conversation.ts`
- `frontend/src/components/Chat/TaskSubagentCard.tsx`

### Tests already confirmed as high-signal

- `src-tauri/src/infrastructure/agents/claude/mod_tests.rs`
- `src-tauri/src/infrastructure/agents/claude/claude_code_client_tests.rs`
- `src-tauri/src/infrastructure/agents/claude/agent_config/tests.rs`
- `src-tauri/src/application/chat_resumption_tests.rs`
- `src-tauri/src/application/chat_service/chat_service_handlers_tests.rs`
- `src-tauri/src/infrastructure/sqlite/sqlite_chat_conversation_repo_tests.rs`
- `frontend/src/types/agent-profile.test.ts`
- `frontend/src/api/chat.test.ts`
- `frontend/src/hooks/useIdeationModelSettings.test.ts`

## Highest-value next edits after compaction

1. Convert `specs/codex-cli-contract.md` into actual coding tasks for phase 1 harness abstraction.
2. Translate the contract into concrete DB migration tasks and replay fixture tasks.
3. Keep `AGENTS.md` tracker aligned with the contract as runtime work begins.
4. Commit and push each major implementation milestone separately.
