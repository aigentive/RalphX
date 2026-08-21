# Codex CLI Multi-Harness Spec

Status: implemented baseline; retained as the original parity map. Current runtime contracts live in `.claude/rules/multi-harness.md`, `.claude/rules/delegation-topology.md`, and `docs/architecture/agent-harnesses.md`.

## 1. Goal

Add Codex CLI with the highest practical feature parity to the current Claude-based RalphX architecture, starting with core functionality:

- Codex-backed ideation
- Codex-backed ideation verification
- Codex-backed subagent/delegation support
- provider-neutral worker/reviewer/merger routing

Initial operator expectation:

- Codex is selectable as a harness wherever RalphX currently selects agent/model behavior
- RX-native Team and delegation operate across supported Claude and Codex runtimes
- all raw events from Claude and Codex are preserved
- higher-level RalphX consumers use a unified provider-neutral event contract

## 2. Required success milestones

### M0. Research baseline

- local Codex doc snapshot committed under `docs/ai-docs/codex-cli/`
- Reefagent Codex integration report committed
- Reefagent raw Codex event shapes documented
- cross-session tracker added to `AGENTS.md`

### M1. First usable parity slice

- ideation sessions can run on Codex
- ideation verification can run on Codex
- Codex specialists / verifier delegation works via vendor-native subagents
- raw event capture and parsed event normalization work end-to-end
- Claude remains default harness and can still run execution lanes

### M2. Full harness routing surface

- harness is configurable per lane:
  - ideation
  - ideation verification
  - task execution
  - review
  - re-execution
  - merge conflict handling
- settings exist in:
  - `ralphx.yaml`
  - env overrides
  - persisted DB defaults
  - per-project settings
  - frontend settings UI

### M3. Execution-lane parity

- worker / reviewer / merger flows can run on Codex where supported
- recovery, reconciliation, capacity control, and queue semantics remain correct
- all current Claude-compatible regression expectations have Codex-aware equivalents

## 3. Non-goals for phase 1

- reintroducing removed vendor-specific Team persistence or process semantics
- forcing Codex into Claude plugin-dir semantics
- moving all execution lanes to Codex immediately
- replacing raw vendor logs with only normalized logs

## 4. Confirmed vendor facts that affect design

### Codex docs

Confirmed from official docs:

- Codex has a first-class CLI, config file, rules, AGENTS.md support, hooks, MCP, skills, subagents, model selection, and non-interactive `codex exec`.
- Official model guidance recommends `gpt-5.4` for most tasks and `gpt-5.4-mini` for lighter tasks and subagents.
- Official config supports `approval_policy`, `mcp_servers.*`, and `model_reasoning_effort`.
- Official docs and the locally installed Codex binary are already divergent.

### Local runtime

Confirmed locally:

- installed binary here reports `codex --version` `0.1.2505172129`
- local help surface is materially smaller than the current official docs

Design consequence:

- RalphX must use capability detection and version gating for Codex

### Reefagent reference

Confirmed locally:

- Reefagent already integrates Codex via a dedicated provider layered over shared CLI-agent process/session utilities
- Reefagent injects Codex MCP via per-run inline config overrides
- Reefagent logs raw Codex events and separately normalizes them
- Reefagent defaults Codex to `gpt-5.4` with `xhigh` reasoning in config

## 5. RalphX parity scope to cover

The Codex project is not only “spawn another binary.” It touches every place RalphX currently assumes Claude-specific semantics.

### 5.1 Spawn layer

Must audit and either abstract or replace every place that assumes:

- `claude` binary name
- Claude plugin-dir resolution
- Claude-specific CLI flags
- Claude-specific resume/session behavior
- provider-specific subagent process topology leaking into RalphX orchestration
- Claude-only availability checks

Concrete RalphX files already confirmed:

- `src-tauri/src/infrastructure/agents/claude/mod.rs`
- `src-tauri/src/infrastructure/agents/claude/claude_code_client.rs`
- `src-tauri/src/application/chat_service/chat_service_context.rs`
- `src-tauri/src/http_server/handlers/coordination/**`
- `src-tauri/src/application/memory_orchestration.rs`
- `src-tauri/src/lib.rs`

### 5.2 Agent definitions and prompt source of truth

Current RalphX source of truth is effectively split across:

- plugin agent markdown frontmatter
- `ralphx.yaml`
- MCP allowlists
- backend spawn logic

Need to decide:

- keep plugin markdown as canonical and derive Codex config from it, or
- introduce a new shared agent-definition source and generate Claude + Codex assets from it

Target requirement:

- one logical agent definition model
- multiple harness renderers

### 5.3 Harness selection and settings

Need configurable harness + model + effort by lane.

Required routing dimensions:

- global default harness
- project default harness
- per-lane harness
- model override
- effort override

Required lane granularity:

- ideation
- ideation verification
- task execution
- review
- re-execution
- merge conflict handling

### 5.4 Event model

Current RalphX consumers likely assume Claude-style stream payloads for:

- chat UI
- activity stream
- run state transitions
- tool-call rendering
- queue telemetry
- recovery logic
- provider error detection

Target requirement:

- `RawHarnessEvent` persisted unchanged
- `NormalizedAgentEvent` emitted to the rest of RalphX

Normalized event families should cover at least:

- run started
- run resumed
- assistant text chunk / message
- tool call started
- tool call completed
- tool call failed
- subagent started
- subagent completed
- usage update
- run completed
- run failed
- run cancelled

### 5.5 Recovery / reconciliation / queueing

All current Claude assumptions must be audited in:

- chat resumption
- startup recovery
- paused queue relaunch
- stop/pause semantics
- running-agent registry
- stale session handling
- provider error classification
- capacity admission
- reconciliation handlers
- merge retry and watchdog flows

Codex-specific questions:

- what is the durable provider session id
- how does resume behave after process death
- what failures are recoverable
- what failures should clear the stored provider session id
- what failures are provider-blocking vs harness-blocking

Concrete RalphX files already confirmed:

- `src-tauri/src/application/startup_jobs.rs`
- `src-tauri/src/application/chat_resumption.rs`
- `src-tauri/src/application/reconciliation/handlers/execution.rs`
- `src-tauri/src/application/reconciliation/handlers/merge.rs`
- `src-tauri/src/commands/execution_commands/settings.rs`
- `src-tauri/src/commands/execution_commands/lifecycle.rs`
- `src-tauri/src/application/task_transition_service.rs`

### 5.6 MCP / internal tools

Claude path today:

- plugin-dir
- prompt frontmatter tool contract
- `ralphx.yaml` grants
- Claude-side MCP allowlists

Codex target:

- generated per-run `mcp_servers.*` config
- tool allowlists via Codex config
- HTTP/streamable HTTP or stdio transport chosen explicitly

Need:

- one logical RalphX tool grant layer
- one Codex translation layer
- one Claude translation layer

### 5.7 Subagents

Provider runtimes expose different native subagent mechanisms. RalphX normalizes them
behind RX-native delegation and Team surfaces rather than retaining a vendor-specific
team process layer.

Need provider-neutral delegation abstraction:

- request subagent
- wait on subagent
- resume subagent
- map parent/child lineage
- collect raw and normalized child events

### 5.8 UI / API surface

Need all user-visible and API-visible harness metadata to be explicit:

- selected harness
- selected model
- reasoning effort
- provider session id
- raw run status
- degraded capability warnings

Affected surfaces likely include:

- ideation settings
- execution settings
- task start controls
- run detail panels
- activity stream widgets
- chat widgets
- diagnostics

Concrete RalphX constraints already confirmed:

- agent profile schemas and APIs are named `claudeCode`
- ideation model UI only offers `inherit|sonnet|opus|haiku`
- chat-facing schemas and widgets consume `claudeSessionId`
- general model labels and capability presentation are Claude-only

## 6. Required architectural outcome

### 6.1 Introduce a first-class harness layer

Target logical interface:

- `AgentHarnessKind`
- `HarnessCapabilities`
- `HarnessSpawnRequest`
- `HarnessSpawnResult`
- `HarnessResumeRequest`
- `HarnessAvailability`
- `HarnessRawEvent`
- `HarnessNormalizedEvent`

Likely implementations:

- `ClaudeHarness`
- `CodexHarness`

### 6.2 Keep ChatService-level orchestration provider-neutral

The current `ClaudeChatService` naming is a code smell for multi-harness support.

Target direction:

- shared `AgentHarnessService` or `UnifiedHarnessChatService`
- harness-specific command builders and stream parsers beneath it

### 6.3 Centralize agent definitions

Target logical definition should include:

- agent id
- lane / purpose
- base instructions
- tool grants
- MCP grants
- model defaults by harness
- effort defaults by harness
- sandbox / approval defaults by harness
- subagent policy
- RX-native Team capability

Possible output targets:

- Claude plugin agent files
- Claude YAML fragments
- Codex config fragments
- direct Codex spawn-time overrides

## 7. RalphX audit buckets that must be covered before implementation

This is the implementation-prep checklist. Every item needs either abstraction, migration, or an intentional Codex limitation.

### 7.1 Backend spawn and runtime

- `src-tauri/src/application/chat_service/**`
- `src-tauri/src/application/chat_resumption.rs`
- `src-tauri/src/application/startup_jobs.rs`
- `src-tauri/src/application/reconciliation/**`
- `src-tauri/src/application/task_transition_service.rs`
- `src-tauri/src/application/task_scheduler_service.rs`
- `src-tauri/src/commands/execution_commands/**`
- `src-tauri/src/commands/unified_chat_commands.rs`
- `src-tauri/src/commands/ideation_commands/**`
- `src-tauri/src/http_server/handlers/ideation/**`
- `src-tauri/src/http_server/handlers/coordination/**`
- `src-tauri/src/http_server/handlers/session_linking/**`
- `src-tauri/src/infrastructure/agents/claude/**`

### 7.2 Config and persistence

- `ralphx.yaml`
- execution defaults seeding
- env override resolution
- DB execution settings tables / repos / migrations
- per-project settings repos / commands
- agent profile APIs

Concrete constraints already confirmed:

- `ralphx.yaml` only has a first-class `claude:` runtime block today
- ideation effort persistence stores only `primary_effort` and `verifier_effort`
- ideation model persistence stores only Claude-era aliases plus verifier/ideation subagent buckets
- chat conversation persistence stores `claude_session_id`
- runtime persistence stores `last_effective_model` and running-agent `model`, but not harness kind

### 7.3 Frontend

- settings schemas and transforms
- settings UI components
- ideation start controls
- task execution controls
- chat/event rendering types
- activity stream rendering
- diagnostics surfaces

Concrete files already confirmed:

- `frontend/src/types/agent-profile.ts`
- `frontend/src/types/settings.ts`
- `frontend/src/components/settings/IdeationModelSection.tsx`
- `frontend/src/api/chat.ts`
- `frontend/src/types/chat-conversation.ts`
- `frontend/src/components/Chat/TaskSubagentCard.tsx`

### 7.4 Tests

- Claude process spawn tests
- stream parser tests
- recovery tests
- queue/pause/stop tests
- execution settings tests
- ideation verification tests
- UI transform and rendering tests

Concrete high-signal suites already confirmed:

- `src-tauri/src/infrastructure/agents/claude/mod_tests.rs`
- `src-tauri/src/infrastructure/agents/claude/claude_code_client_tests.rs`
- `src-tauri/src/infrastructure/agents/claude/agent_config/tests.rs`
- `src-tauri/src/application/chat_resumption_tests.rs`
- `src-tauri/src/application/chat_service/chat_service_handlers_tests.rs`
- `src-tauri/src/infrastructure/sqlite/sqlite_chat_conversation_repo_tests.rs`
- `frontend/src/types/agent-profile.test.ts`
- `frontend/src/api/chat.test.ts`
- `frontend/src/hooks/useIdeationModelSettings.test.ts`

## 8. Proposed logical config model

RalphX needs a logical config layer that is not Claude-shaped.

### 8.1 Harness enum

- `claude`
- `codex`

### 8.2 Logical effort enum

- `low`
- `medium`
- `high`
- `xhigh`

Harness translation:

- Claude may not support the same effort surface everywhere
- Codex official config supports `xhigh`
- unsupported combinations should downgrade explicitly and log the downgrade

### 8.3 Lane settings

Each lane should be able to store:

- harness
- model
- effort
- approval policy
- sandbox policy
- fallback harness optional

## 9. Codex-specific target defaults

For the user-requested starting point:

- ideation:
  - harness: `codex`
  - model: `gpt-5.4`
  - effort: `xhigh`
- ideation verification:
  - harness: `codex`
  - model: `gpt-5.4-mini`
  - effort: `medium` unless verification quality requires `high`
- task execution pipeline:
  - keep current Claude defaults initially

## 10. Logging and auditability requirements

Must record per run:

- RalphX run id
- harness kind
- binary version / detected capabilities
- logical model + resolved vendor model
- logical effort + resolved vendor effort
- approval policy
- sandbox policy
- provider session id
- raw event log path
- prompt capture path
- child/subagent lineage

Raw logging rules:

- keep current Claude raw logs
- add Codex raw logs
- do not normalize destructively

## 11. Regression requirements

Need coverage for:

- harness availability detection
- spawn argument construction
- MCP grant translation
- raw Codex event parsing
- raw Claude event parsing into the same normalized contract
- run completion / failure handling
- queue, pause, stop, and resume behavior
- stale session / stale run recovery
- settings persistence and env overrides
- frontend transforms and widgets using normalized events only

Replay-based testing should be added:

- feed captured Claude raw events into the parser
- feed captured Codex raw events into the parser
- assert identical normalized downstream behavior where logically equivalent

## 12. Known hard problems

### 12.1 RX-native Team and provider subagents

Decision implemented:

- RX-native Team uses the provider-neutral delegation contract for supported Claude and Codex runtimes.
- Provider-native differences remain behind harness adapters and normalized events.

### 12.2 Plugin-dir vs config-driven MCP

Claude and Codex have different integration seams.

Design consequence:

- MCP grants and tool contracts must be provider-neutral in RalphX core
- vendor translation happens at the harness boundary

### 12.3 Version skew

Official Codex docs and local binary differ today.

Design consequence:

- capability detection is required
- hard minimum supported Codex version may be required for rollout

### 12.4 Claude-specific data contract leakage

Already confirmed in RalphX:

- `claude_session_id` in persisted chat conversations
- `claudeCode` in agent-profile API/types
- Claude-only model alias enums in settings UI
- Claude-specific parser/event contracts in backend and frontend

Design consequence:

- this effort needs a deliberate rename and compatibility strategy, not just a second spawn path

## 13. Concrete migration ordering by blast radius

### Abstract first

- harness enum and capability detection
- provider-neutral run/session identifiers
- normalized event contract
- provider-neutral MCP/tool grant model
- provider-neutral prompt / agent-definition model

### Next layer

- backend spawn service split away from Claude naming
- Codex availability checks
- Codex raw parser and replay tests
- ideation / verification harness routing

### Rename or compatibility layer after abstractions exist

- `claude_session_id` -> provider-neutral session id plus legacy compatibility
- `claudeCode` profile/config surface -> harness-neutral client config surface
- Claude-only model aliases in frontend settings

### Final convergence

- RX-native Team execution flows
- worker/reviewer/merger harness parity
- residual docs/example cleanup after runtime parity exists

## 14. Current recommended sequence after discovery is complete

1. Land harness abstraction and normalized event contract.
2. Land Codex availability + spawn plumbing behind a feature flag.
3. Land Codex ideation and verification only.
4. Land settings persistence and UI for per-lane harness selection.
5. Land execution-lane Codex support only after recovery/reconciliation parity is proven.

## 15. Documents linked to this effort

- `AGENTS.md` Codex CLI Project Tracker
- `docs/ai-docs/codex-cli/`
- `docs/ai-docs/reefagent-codex-cli.md`
- `docs/ai-docs/reefagent-codex-responses.md`
- `specs/codex-cli-contract.md`

## 16. Lane-by-lane settings, storage, and UI matrix

RalphX currently has no single “agent harness settings” model. Different areas store different Claude-shaped slices.

### 16.1 Ideation primary lane

Current state:

- YAML agent config via `ralphx.yaml` agent entries such as `ralphx-ideation`
- DB project/global row in `ideation_model_settings.primary_model`
- DB project/global row in `ideation_effort_settings.primary_effort`
- last run capture in `ideation_sessions.last_effective_model`
- runtime resolution in:
  - `src-tauri/src/commands/ideation_commands/ideation_commands_model.rs`
  - `src-tauri/src/commands/ideation_commands/ideation_commands_effort.rs`
  - `src-tauri/src/application/chat_service/chat_service_context.rs`
- UI in `frontend/src/components/settings/IdeationModelSection.tsx`

Required Codex parity:

- add explicit `harness`
- allow full vendor model ids, not only Claude aliases
- store logical effort separately from vendor translation
- expose both logical selection and effective resolved vendor settings

### 16.2 Ideation verification lane

Current state:

- YAML agent config for `ralphx-plan-verifier`
- DB row in `ideation_model_settings.verifier_model`
- DB row in `ideation_effort_settings.verifier_effort`
- UI in the same ideation settings section

Required Codex parity:

- independent harness selection from primary ideation
- independent model and effort selection
- explicit compatibility warnings when verifier flow requires a Claude-only feature

### 16.3 Ideation subagent lane

Current state:

- DB row in `ideation_model_settings.ideation_subagent_model`
- no parallel effort bucket today
- prompt injection in `chat_service_context.rs` assumes Claude Task tool model ceilings

Required Codex parity:

- treat as a true lane with `harness`, `model`, `effort`
- allow Codex-native subagents for Codex runs
- preserve Claude Task/Agent behavior when Claude remains selected

### 16.4 Verification subagent lane

Current state:

- DB row in `ideation_model_settings.verifier_subagent_model`
- no dedicated effort setting
- runtime selection in `chat_service_context.rs`

Required Codex parity:

- same lane contract as ideation subagents
- first-class delegation capability check per harness

### 16.5 Execution pipeline lanes

Current state:

- execution worker/reviewer/merge model behavior is mostly driven by YAML agent config in `ralphx.yaml`
- project/global execution settings do not yet expose harness/model/effort by lane
- frontend project settings only expose a single Claude-shaped `model` plus `allow_opus_upgrade`

Required Codex parity:

- configurable lanes:
  - execution
  - review
  - re-execution
  - merge conflict handling
- configurable at:
  - YAML defaults
  - env override layer
  - persisted global defaults
  - persisted per-project overrides
  - area/lane-specific UI

### 16.6 Chat conversations and run history

Current state:

- conversation persistence stores provider session id only in `chat_conversations.claude_session_id`
- event payloads and APIs return `claudeSessionId` / `claude_session_id`
- agent runs persist model info but not harness kind

Required Codex parity:

- provider-neutral session storage:
  - `provider_session_id`
  - `provider_session_kind` or `harness`
- run history must persist:
  - harness
  - logical model
  - resolved vendor model
  - logical effort
  - resolved vendor effort
  - approval policy
  - sandbox mode

## 17. Current Claude-specific data contracts that must be migrated

### 17.1 Persistence and repositories

Confirmed Claude-shaped contracts:

- `src-tauri/crates/ralphx-domain/src/entities/chat_conversation.rs`
  - field `claude_session_id`
  - methods `set_claude_session_id()` and `has_claude_session()`
- `src-tauri/crates/ralphx-domain/src/repositories/chat_conversation_repository.rs`
  - `update_claude_session_id()`
  - `clear_claude_session_id()`
- `src-tauri/src/infrastructure/sqlite/sqlite_chat_conversation_repo.rs`
- `src-tauri/src/infrastructure/memory/memory_chat_conversation_repo.rs`
- initial SQLite schema in `src-tauri/src/infrastructure/sqlite/migrations/v1_initial_schema.rs`
  - column `claude_session_id`
  - index `idx_chat_conversations_claude_session`

Migration consequence:

- this needs a forward-only compatibility migration, not a silent semantic reuse of the old column name

### 17.2 Ideation model and effort contracts

Confirmed Claude-shaped contracts:

- `src-tauri/crates/ralphx-domain/src/ideation/model_settings.rs`
  - `ModelLevel = inherit|sonnet|opus|haiku`
- `src-tauri/crates/ralphx-domain/src/ideation/effort_settings.rs`
  - `EffortLevel = low|medium|high|max|inherit`
  - explicitly documented as Claude `--effort`
- `src-tauri/src/commands/ideation_commands/ideation_commands_model.rs`
  - resolution hardcodes YAML agent config and default `"sonnet"`
- `src-tauri/src/commands/ideation_commands/ideation_commands_effort.rs`
  - resolution derives from Claude config stack

Migration consequence:

- current enums cannot represent `gpt-5.4` or `xhigh`
- lane settings need a provider-neutral schema above vendor adapters

### 17.3 Agent profiles and capability surface

Confirmed Claude-shaped contracts:

- `src-tauri/crates/ralphx-domain/src/agents/agent_profile.rs`
  - `Model` enum only `opus|sonnet|haiku`
  - `ClaudeCodeConfig`
  - `AgentProfile.claude_code`
- `src-tauri/src/commands/agent_profile_commands.rs`
  - response payload field `claude_code`
- `frontend/src/types/agent-profile.ts`
  - `ModelSchema = opus|sonnet|haiku`
  - `ClaudeCodeConfigSchema`
  - `AgentProfile.claudeCode`
- `src-tauri/crates/ralphx-domain/src/agents/capabilities.rs`
  - only `ClientCapabilities::claude_code()` and mock currently exist

Migration consequence:

- agent-profile APIs either need a compatibility layer or a v2 shape
- harness-neutral capability listing must exist before UI can expose Codex safely

### 17.4 Event and chat payloads

Confirmed Claude-shaped contracts:

- `src-tauri/src/application/chat_service/chat_service_types.rs`
  - `AgentRunCompletedPayload.claude_session_id`
  - comments refer to Claude model ids
- `src-tauri/src/commands/unified_chat_commands.rs`
  - serialized `claude_session_id`
- `frontend/src/types/chat-conversation.ts`
  - `claudeSessionId`
  - `SendContextMessageResponse.claude_session_id`
- `frontend/src/api/chat.ts`
  - child-session and active-state helpers assume current Claude-flavored payloads

Migration consequence:

- normalized run completion payloads need provider-neutral session metadata
- frontend types should stop naming any vendor directly

## 18. Normalized event contract draft

Provider-neutral consumers should stop depending on Claude stream message shapes and instead consume a normalized event stream.

### 18.1 Raw persistence contract

Persist unchanged:

- `harness`
- `run_id`
- `conversation_id`
- `provider_session_id`
- `raw_event_kind`
- raw payload bytes/string
- ingest timestamp
- parse status

### 18.2 Normalized event families

Minimum stable contract:

- `RunStarted`
  - `run_id`
  - `conversation_id`
  - `context_type`
  - `context_id`
  - `harness`
  - `effective_model_id`
  - `effective_model_label`
- `RunResumed`
  - `provider_session_id`
- `AssistantTextDelta`
  - `text`
  - `seq`
- `AssistantMessage`
  - full text / block payload
- `ToolCallStarted`
  - `tool_id`
  - `tool_name`
  - `arguments`
  - `parent_tool_id`
- `ToolCallUpdated`
  - partial status/result
- `ToolCallCompleted`
  - final result
  - error if any
- `SubagentStarted`
  - `subagent_id`
  - `subagent_kind`
  - `model`
  - `teammate_name`
- `SubagentCompleted`
  - `subagent_id`
  - usage totals
- `UsageUpdated`
  - `input_tokens`
  - `cached_input_tokens`
  - `output_tokens`
- `HookEvent`
  - hook name, status, output
- `RunCompleted`
  - `provider_session_id`
  - final usage snapshot
- `RunFailed`
  - classified provider/harness error
- `RunCancelled`

### 18.3 Downstream consumers that should use only normalized events

- chat UI streaming
- task subagent widgets
- activity stream
- run registry updates
- queue telemetry
- recovery bookkeeping
- persisted parsed event traces

### 18.4 Provider-specific parsing remains below the boundary

- Claude parser stays responsible for `stream-json` decoding
- Codex parser stays responsible for JSONL envelope + nested item decoding
- only the normalized output crosses into shared orchestration and UI code

## 19. Prompt, agent, and tool source-of-truth options

This project currently has four coupled Claude-era sources:

- plugin agent markdown frontmatter
- `ralphx.yaml` agent entries
- backend spawn logic
- MCP allowlists/tool translation rules

That should not be duplicated for Codex.

### 19.1 Preferred target

Introduce a provider-neutral agent-definition source that can express:

- agent id
- lane
- prompt body / prompt fragments
- tool grants
- MCP grants
- subagent policy
- harness capability requirements
- per-harness defaults:
  - model
  - effort
  - approval policy
  - sandbox policy

### 19.2 Output renderers

Generate or derive from the shared source:

- Claude plugin agent markdown
- Claude runtime YAML fragments or runtime spawn options
- Codex run config fragments or CLI overrides
- UI/settings metadata

### 19.3 Acceptable transitional state

Phase 1 may keep Claude markdown files as the user-facing authored prompt source if RalphX adds a build step or runtime translation layer that derives Codex-compatible prompts and tool grants from them.

Constraint:

- there must still be one logical source of truth for effective tool/MCP grants

## 20. Implementation gates before coding begins

This discovery pass should be considered complete only when these are explicitly answered in-doc:

- which persisted schema changes happen first and which compatibility aliases remain temporarily
- how per-lane harness selection is represented in YAML, env, DB, and frontend
- how Codex MCP is injected and how its allowlist is derived from RalphX logical grants
- how normalized events map to existing UI widgets
- how startup recovery and stale-session repair behave per harness
- which execution-lane flows are explicitly unsupported in phase 1

Current answer status:

- persisted schema risk: identified
- settings matrix: identified
- MCP translation direction: identified
- normalized event boundary: identified
- recovery/resume seam inventory: identified
- RX-native Team provider contract: implemented

Remaining implementation-planning work after this document:

- choose exact compatibility migration strategy
- choose exact config schema shapes
- choose exact frontend settings UX
