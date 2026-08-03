# Claude CLI Spawning System — Claude-Specific Architecture Reference

## Overview

This file is intentionally Claude-centric. RalphX now has a provider-neutral harness layer above this transport, so use this document for Claude-specific spawning/plugin/MCP details, not as the universal runtime contract for Codex or future harnesses.

RalphX spawns Claude CLI processes as child OS processes via `tokio::process::Command`. Each agent type gets a dynamically-constructed CLI invocation with agent-specific tool restrictions, MCP configuration, model selection, settings profiles, and permission handling. A TypeScript MCP server (`ralphx-mcp-server`) acts as a stdio proxy between Claude and the Tauri backend HTTP server at `:3847`.

```
┌─────────────┐     ┌──────────────┐     ┌──────────────────┐     ┌──────────────┐
│ State Machine│────▶│  Spawner      │────▶│ ClaudeCodeClient │────▶│ claude CLI    │
│ (statig)     │     │ (bridge)      │     │ (tokio::Command) │     │ (OS process)  │
└─────────────┘     └──────────────┘     └──────────────────┘     └──────┬───────┘
                                                                         │ stdio
                                                                   ┌─────▼────────┐
                                                                   │ MCP Server   │
                                                                   │ (Node.js)    │
                                                                   └──────┬───────┘
                                                                          │ HTTP
                                                                   ┌──────▼───────┐
                                                                   │ Tauri :3847  │
                                                                   │ (Axum)       │
                                                                   └──────────────┘
```

## Layer Architecture

### 1. Domain Layer — Agent Abstractions

**File:** `src-tauri/src/domain/agents/agentic_client.rs`

```rust
#[async_trait]
pub trait AgenticClient: Send + Sync {
    async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle>;
    async fn stop_agent(&self, handle: &AgentHandle) -> AgentResult<()>;
    async fn wait_for_completion(&self, handle: &AgentHandle) -> AgentResult<AgentOutput>;
    async fn send_prompt(&self, handle: &AgentHandle, prompt: &str) -> AgentResult<AgentResponse>;
    fn stream_response(&self, handle: &AgentHandle, prompt: &str) -> Pin<Box<dyn Stream<...>>>;
    fn capabilities(&self) -> &ClientCapabilities;
    async fn is_available(&self) -> AgentResult<bool>;
}
```

**Key Types** (`src-tauri/src/domain/agents/types.rs`):

| Type | Purpose |
|------|---------|
| `AgentConfig` | role, prompt, working_directory, plugin_dir, agent, model, max_tokens, timeout_secs, env |
| `AgentHandle` | client_type, role, id (UUID) |
| `AgentRole` | Worker, QaPrep, QaRefiner, QaTester, Reviewer, Supervisor, Custom(String) |
| `ClientType` | ClaudeCode, Mock |
| `AgentOutput` | success, content, exit_code, duration_ms |
| `ClientCapabilities` | supports_shell/filesystem/streaming/mcp, max_context_tokens, available_models |

### 2. Infrastructure Layer — Process Spawning

#### 2a. ClaudeCodeClient — Direct CLI Spawning

**File:** `src-tauri/src/infrastructure/agents/claude/claude_code_client.rs`

Two spawn modes:

| Mode | Method | Use Case |
|------|--------|----------|
| Fire-and-forget | `spawn_agent()` → `wait_for_completion()` | Background agents (ralphx-utility-session-namer, ralphx-project-analyzer, memory capture/maintenance) |
| Streaming | `spawn_agent_streaming()` → returns `Child` | Interactive sessions (ExecutionChatService handles stream) |

**Process tracking:** Global `PROCESSES: Mutex<HashMap<String, (Child, Instant)>>` tracks all spawned processes by handle ID for stop/wait operations.

**CLI binary resolution** (`claude_code_client.rs:92-93`):
```rust
let cli_path = which::which("claude").unwrap_or_else(|_| PathBuf::from("claude"));
```

Also supports PATH lookup plus hardcoded fallback paths (`/opt/homebrew/bin/claude`, `/usr/local/bin/claude`).

#### 2b. AgenticClientSpawner — State Machine Bridge

**File:** `src-tauri/src/infrastructure/agents/spawner.rs`

Bridges the state machine's `AgentSpawner` trait to `AgenticClient`:

```rust
impl AgentSpawner for AgenticClientSpawner {
    async fn spawn(&self, agent_type: &str, task_id: &str);
    async fn spawn_background(&self, agent_type: &str, task_id: &str);
    async fn wait_for(&self, agent_type: &str, task_id: &str);
    async fn stop(&self, agent_type: &str, task_id: &str);
}
```

**Key behaviors:**
- **B5 dedup** (`spawner.rs:189-200`): Prevents duplicate spawns for the same task_id
- **Execution state gating** (`spawner.rs:202-240`): Checks `can_start_task()` before spawning — blocks if paused or at max_concurrent
- **Running count management** (`spawner.rs:234-239`): Increments before spawn, emits `execution:spawn_blocked` event on rejection
- **Per-task working directory** (`spawner.rs:156-183`): Resolves worktree path for Worktree git mode, falls back to project directory
- **Supervisor events** (`spawner.rs:112-135`): Emits `task_start`, `tool_call`, `error` via EventBus

## CLI Argument Construction

### Standard Arguments (every spawn)

Built in `build_cli_args()` (`claude_code_client.rs:348-452`) and `spawn_agent()` (`claude_code_client.rs:120-255`):

| Flag | Value | Source |
|------|-------|--------|
| `-p` | Agent prompt | `AgentConfig.prompt` |
| `--output-format` | `stream-json` | Hardcoded |
| `--verbose` | (flag) | Required for stream-json with -p |
| `--disable-slash-commands` | (flag) | Avoids startup parser crashes |
| `--plugin-dir` | `<path>` | `AgentConfig.plugin_dir` → `resolve_plugin_dir()` |
| `--agent` | `ralphx:<name>` | `AgentConfig.agent` (fully-qualified) |
| `--mcp-config` | `<temp_path>` | Dynamic per-agent MCP config (see below) |
| `--disallowedTools` | MCP server/tool patterns | Effective global/project RalphX deny policy |
| `--tools` | CSV of CLI tools | `get_allowed_tools(agent_name)` from `config/ralphx.yaml` |
| `--allowedTools` | CSV of pre-approved | `get_preapproved_tools(agent_name)` — MCP + CLI, no prompts |
| `--model` | `haiku`/`sonnet`/`opus` | Explicit override → per-agent default from `config/ralphx.yaml` |
| `--max-tokens` | number | Optional from `AgentConfig.max_tokens` |
| `--permission-prompt-tool` | `mcp__ralphx__permission_request` | From `config/ralphx.yaml` `claude.permission_prompt_tool` |
| `--permission-mode` | `default` | From `config/ralphx.yaml` `claude.permission_mode` |
| `--settings` | JSON string | Agent-specific or global settings profile |
| `--setting-sources` | CSV | Optional override from `config/ralphx.yaml` |

### Conditional Arguments

| Flag | Condition | Purpose |
|------|-----------|---------|
| `--resume <session_id>` | Session recovery | Continue existing conversation |
| `--dangerously-skip-permissions` | `claude.dangerously_skip_permissions: true` | Skip all permission checks |
| `--debug-file <path>` | `build_base_cli_command()` path | Post-mortem analysis for silent exits |
| `--append-system-prompt-file <path>` | Non-native agent mode | Inject agent behavior via system prompt |

### Environment Variables Set on Process

| Variable | Value | Purpose |
|----------|-------|---------|
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | `1` | Reduces non-essential API calls |
| `DEBUG` | `true` | Enables debug output |
| `CLAUDE_PLUGIN_ROOT` | plugin dir path | Plugin root for MCP server resolution |
| `RALPHX_PROJECT_ID` | project UUID | Passed to agents for project scoping |
| Custom env | from `AgentConfig.env` | Task-specific variables |

## Configuration System

### config/ralphx.yaml — Shared Runtime Config

**File:** `config/ralphx.yaml` — embedded at compile time via `include_str!`
**Parser:** `src-tauri/src/infrastructure/agents/claude/agent_config/mod.rs`

```yaml
tool_sets:
  base_tools: [Read, Grep, Glob, Bash, WebFetch, WebSearch, Skill]

claude:
  mcp_server_name: ralphx
  permission_mode: default
  permission_prompt_tool: permission_request    # auto-prefixed → mcp__ralphx__permission_request
  dangerously_skip_permissions: false
  append_system_prompt_file: true
  settings_profile: default
  settings_profiles:
    default:
      sandbox: { enabled: false }
    z_ai:
      extends: default
      env:
        ANTHROPIC_BASE_URL: https://api.z.ai/api/anthropic

agents:
  - name: ralphx-ideation
    model: opus
    system_prompt_file: agents/ralphx-ideation/claude/prompt.md
    tools:
      extends: base_tools
      include: [Task]
    mcp_tools: [create_task_proposal, update_task_proposal, ...]
    preapproved_cli_tools: [Task(Plan)]
```

**Config loading** (`agent_config/mod.rs:444-475`):
1. Try `RALPHX_CONFIG_PATH` env var
2. Try `<cargo_manifest>/../config/ralphx.yaml` from filesystem
3. Fall back to embedded config (compile-time `include_str!`)

**Resolved at startup via `OnceLock`** — loaded once, immutable after.

### Three-Layer Tool Allowlist

| Layer | CLI Flag | Source | Purpose |
|-------|----------|--------|---------|
| CLI tools | `--tools` | `agent.tools.extends` + `include` | Whitelist of built-in Claude tools |
| MCP tools | `--allowedTools` | `agent.mcp_tools[]` (auto-prefixed) | RalphX MCP tools the agent can call |
| Pre-approved | `--allowedTools` | CLI tools + MCP tools + `preapproved_cli_tools` | No permission prompts |

**Tool resolution** (`agent_config/mod.rs:144-167`):
```
tools.extends: "base_tools" → lookup tool_sets["base_tools"]
tools.include: ["Write", "Edit", "Task"] → append
tools.mcp_only: true → empty CLI tools (agent uses only MCP)
```

**Pre-approved tools** (`agent_config/mod.rs:514-549`):
```
MCP tools → prefixed as mcp__ralphx__<name>
CLI tools → passed as-is
preapproved_cli_tools → appended (e.g., Task(Plan))
Memory skills → only for ralphx-memory-maintainer and ralphx-memory-capture agents
```

### Settings Profiles

**File:** `agent_config/mod.rs:292-319`

```
claude.settings_profiles.default → base settings
claude.settings_profiles.z_ai → extends: default + overrides
claude.settings_profile_defaults → merged into every profile
```

**Resolution order:**
1. Agent-specific `settings_profile` field in YAML
2. `RALPHX_CLAUDE_SETTINGS_PROFILE_<AGENT_NAME>` env var (per-agent override)
3. Global `RALPHX_CLAUDE_SETTINGS_PROFILE` env var
4. `claude.settings_profile` in YAML
5. `claude.settings_profiles.default` if exists
6. `claude.settings` legacy field

**Profile inheritance** (`agent_config/mod.rs:329-372`): Profiles can `extends: [base1, base2]` with cycle detection.

**Runtime env overrides** (`agent_config/mod.rs:424-442`): `settings.env` keys can be overridden by `RALPHX_<KEY>` environment variables.

## Dynamic MCP Config Generation

**File:** `src-tauri/src/infrastructure/agents/claude/mod.rs:319-424`

Each agent spawn creates a temporary MCP config file that:
1. Reads the base config from `plugins/app/.mcp.json`
2. Injects `--agent-type=<short_name>` into the MCP server args
3. Writes to a temp file with UUID to avoid race conditions between parallel spawns
4. Passes via `--mcp-config <temp_path>` without strict isolation, so Claude also loads its native user/project/local MCP layers

**Why `--agent-type` as CLI arg**: Claude CLI does NOT pass its environment variables to MCP servers it spawns. The `--agent-type` CLI arg is the only reliable way to communicate the agent type to the MCP server process.

```json
{
  "mcpServers": {
    "ralphx": {
      "type": "stdio",
      "command": "node",
      "args": ["/path/to/ralphx-mcp-server/build/index.js", "--agent-type", "ralphx-execution-worker"]
    }
  }
}
```

## MCP Server — TypeScript Proxy Layer

**File:** `plugins/app/ralphx-mcp-server/src/index.ts`
**Transport:** Stdio (JSON-RPC 2.0 via `@modelcontextprotocol/sdk`)

### Request Flow

```
Claude CLI ──stdio──▶ MCP Server ──HTTP──▶ Tauri :3847
                         │
                    ┌────▼────┐
                    │ Filter  │ agent type → tool allowlist
                    │ Validate│ task_id scope enforcement
                    │ Route   │ tool name → HTTP endpoint
                    └─────────┘
```

### Agent Type Detection (`index.ts:42-71`)

Priority:
1. CLI arg: `--agent-type=<type>` (parsed from `process.argv`)
2. Env var: `RALPHX_AGENT_TYPE` (fallback)

### Tool Filtering

**Static registry:** `tools.ts:38-1045` — 60+ MCP tool definitions in `ALL_TOOLS` array.

**Per-agent allowlist:** `tools.ts:1051-1288` — hard-coded mapping: agent type → allowed tool names.

On `ListToolsRequest`: returns only tools in the agent's allowlist.
On `CallToolRequest`: validates `isToolAllowed()` before forwarding.

### HTTP Client (`tauri-client.ts`)

| Method | Usage | Example |
|--------|-------|---------|
| `callTauri(endpoint, args)` | POST `/api/{endpoint}` with JSON body | `create_task_proposal` → `POST /api/create_task_proposal` |
| `callTauriGet(endpoint)` | GET `/api/{endpoint}` | `task_context/{task_id}` → `GET /api/task_context/{id}` |

Default: `http://127.0.0.1:3847` (overridable via `TAURI_API_URL`)

### Task Scope Enforcement (`index.ts:91-134`)

- `RALPHX_TASK_ID` env var defines the task scope
- Tool calls with `task_id` parameter are validated against this scope
- Prevents agents from accessing data for other tasks

### Special Tools

| Tool | File | Protocol |
|------|------|----------|
| `permission_request` | `permission-handler.ts` | POST request → long-poll await (5 min timeout) |
| `ask_user_question` | `question-handler.ts` | POST request → long-poll await (5 min timeout) |

## Tauri HTTP Server (:3847)

**File:** `src-tauri/src/http_server/mod.rs`
**Framework:** Axum
**Binding:** `127.0.0.1:3847` with 5 retry attempts, 250ms delay
**CORS:** All origins allowed

### Endpoint Categories

| Category | Routes | Agent(s) |
|----------|--------|----------|
| Ideation | `create/update/delete_task_proposal`, `list_session_proposals`, `analyze_dependencies` | ralphx-ideation |
| Plans | `create/update_plan_artifact`, `get_session_plan`, `link_proposals_to_plan` | ralphx-ideation |
| Tasks | `update_task`, `add_task_note`, `get_task_details` | ralphx-chat-task |
| Projects | `list_tasks`, `suggest_task` | ralphx-chat-project |
| Reviews | `complete_review`, `get_review_notes`, `approve_task`, `request_task_changes` | reviewer, review-chat |
| Issues | `task_issues/:id`, `mark_issue_*`, `issue_progress/:id` | worker, reviewer |
| Context | `task_context/:id`, `artifact/:id`, `artifact/:id/version/:v`, `artifacts/search` | worker, coder, reviewer |
| Steps | `task_steps/:id`, `start/complete/skip/fail/add_step`, `step_progress`, `step_context`, `sub_steps` | worker |
| Permission | `permission/request`, `permission/await/:id`, `permission/resolve` | All agents (via MCP) |
| Question | `question/request`, `question/await/:id`, `question/resolve` | ralphx-ideation |
| Git/Merge | `git/tasks/:id/complete-merge`, `report-conflict`, `report-incomplete`, `merge-target` | merger |
| Memory | `search/get/upsert_memories`, `mark_memory_obsolete`, `ingest_rule_file` | ralphx-memory-maintainer, ralphx-memory-capture |
| Analysis | `projects/:id/analysis` | ralphx-project-analyzer |

## Agent Name System

**File:** `src-tauri/src/infrastructure/agents/claude/agent_names.rs`

| Constant | Short Name | FQ Name | Usage |
|----------|------------|---------|-------|
| `AGENT_ORCHESTRATOR_IDEATION` | `ralphx-ideation` | `ralphx:ralphx-ideation` | ChatService (Ideation context) |
| `AGENT_WORKER` | `ralphx-execution-worker` | `ralphx:ralphx-execution-worker` | ChatService (TaskExecution) |
| `AGENT_CODER` | `ralphx-execution-coder` | `ralphx:ralphx-execution-coder` | Delegated by worker |
| `AGENT_REVIEWER` | `ralphx-execution-reviewer` | `ralphx:ralphx-execution-reviewer` | ChatService (Review) |
| `AGENT_MERGER` | `ralphx-execution-merger` | `ralphx:ralphx-execution-merger` | ChatService (Merge) |
| `AGENT_SESSION_NAMER` | `ralphx-utility-session-namer` | `ralphx:ralphx-utility-session-namer` | Fire-and-forget (haiku) |
| `AGENT_PROJECT_ANALYZER` | `ralphx-project-analyzer` | `ralphx:ralphx-project-analyzer` | Fire-and-forget (haiku) |
| `AGENT_QA_PREP` | `ralphx-qa-prep` | `ralphx:ralphx-qa-prep` | State machine (Ready) |
| `AGENT_QA_REFINER` | `qa-refiner` | `ralphx:qa-refiner` | State machine (QaRefining) |
| `AGENT_QA_TESTER` | `qa-tester` | `ralphx:qa-tester` | State machine (QaTesting) |

**Name qualification** (`mod.rs:34-47`):
- `qualify_agent_name("worker")` → `"ralphx:worker"`
- `mcp_agent_type("ralphx:worker")` → `"worker"` (strips prefix)

**Spawner mapping** (`agent_names.rs:98-118`): Maps state machine short names to FQ names.

## Stream Processing

This section covers the Claude-native envelope only. The canonical cross-harness thinking lifecycle, capability-probe rules, shared event schema, persistence, frontend behavior, and tests live in [Agent Thinking Capture](agent-thinking-capture.md).

**Files:** `src-tauri/src/infrastructure/agents/claude/stream_processor/{mod.rs,types.rs,parser.rs}`

### Stream Message Types

| Type | Key Fields | Emitted Events |
|------|-----------|----------------|
| `content_block_start` (tool_use) | name, id | `ToolCallStarted` |
| `content_block_delta` (text_delta) | text | `TextChunk` |
| `content_block_delta` (thinking_delta) | text | `Thinking { text, block_index }` |
| `content_block_delta` (input_json_delta) | partial_json | (accumulated) |
| `content_block_stop` | — | `ThinkingSettled` for thinking; `ToolCallCompleted`, `TaskStarted` (if Task tool) |
| `assistant` | content[], session_id | `TextChunk`, `ToolCallCompleted`, `SessionId` |
| `result` | session_id, is_error, cost_usd | `SessionId` |
| `system` | subtype, hook_* | `HookStarted`, `HookCompleted`, `SessionId` |
| `user` | tool_result[] | `ToolResultReceived`, `TaskCompleted` (if Task result) |

### StreamProcessor State Machine

```
process_message(msg) → Vec<StreamEvent>
    ├── Accumulates response_text
    ├── Tracks current_tool_name / current_tool_input (partial tool calls)
    ├── Builds content_blocks[] (interleaved text + tool calls)
    ├── Captures session_id from Result/Assistant/System messages
    └── finish() → StreamResult { response_text, tool_calls, content_blocks, session_id, is_error }
```

### Parent Tool Use ID / Subagent Tracking

`ParsedLine` extracts `parent_tool_use_id` from top-level JSON envelope — propagated to all emitted events for subagent attribution. `is_synthetic` flag distinguishes hook-block user messages.

## Session Recovery

**File:** `src-tauri/src/application/chat_service/chat_service_recovery.rs`

Triggered when a Claude session becomes stale (expired, crashed).

### Recovery Flow

1. **Build replay** — `ReplayBuilder::build_replay()` reconstructs conversation history (100K token budget)
2. **Generate bootstrap prompt** — `build_rehydration_prompt()` combines replay + context + new user message
3. **Spawn fresh session** — `build_command()` + `spawn()` creates new Claude process (no `--resume`)
4. **Process stream** — `process_stream_background()` captures new `session_id` from Result event
5. **Update DB** — `conversation_repo.update_claude_session_id()` stores new session ID

### Resume Session Support

For non-stale sessions, `--resume <session_id>` continues an existing conversation:
- `build_cli_args()` includes `--resume` + `--agent` (critical: `--agent` enforces tool restrictions on resume)
- Session ID captured from stream `Result` event for future resumes

## Provider-Native MCP Inheritance

RalphX never directly rewrites `~/.claude.json` or registers a user-scoped server at app startup. Each launch adds only the required RalphX config and resolved `--disallowedTools` deny patterns; Claude remains authoritative for third-party definitions, trust, approvals, authentication, and native enabled state. The exact Claude user-scoped server ID `ralphx` is reserved RalphX state regardless of definition shape. Startup and launch preflight remove it through the resolved Claude CLI under a process-local lock, force the child `HOME` to the validated discovery root, and rediscover the same provider state after success, non-zero exit, or an explicitly terminated timeout. Only verified absence continues launch; project/local entries, other providers, near names, and `ralphx_internal` remain fail-closed collisions. See [Provider-Native MCP Policy](provider-native-mcp-policy.md).

## Spawn Safety

### Test Environment Detection (`mod.rs:55-69`)

Spawning is blocked when:
- `cfg!(test)` — Rust test harness
- `RUST_TEST_THREADS` env var set
- `RALPHX_TEST_MODE=1` or `true`
- `RALPHX_DISABLE_CLAUDE_SPAWN=1` or `true`

### Execution State Gating (`spawner.rs:202-240`)

Before every spawn:
1. `can_start_task()` — checks `!is_paused() && running_count < max_concurrent`
2. `increment_running()` — atomically increments counter
3. Emits `execution:spawn_blocked` if gated (with reason: `execution_paused` or `max_concurrent_reached`)
4. Emits `execution:status_changed` on successful start

## Plugin Directory Resolution

**File:** `src-tauri/src/infrastructure/agents/claude/mod.rs:746-818`

Priority order:
1. Process-configured bundled/runtime plugin dir (`configure_runtime_plugin_dirs`)
2. RalphX source checkout `plugins/app` (development)
3. RalphX source checkout `ralphx-plugin` (legacy fallback)
4. Relative to executable path (up to 3 levels)

The active target project `working_dir` and `RALPHX_*` env vars are not used for RalphX-owned plugin/runtime root discovery.

## Alternative Spawn Modes

### SpawnableCommand — Stdin Piping

**File:** `src-tauri/src/infrastructure/agents/claude/mod.rs:431-491`

**CLI bug workaround (2.1.38):** `--agent` + `-p "text"` hangs in some scenarios. `SpawnableCommand` pipes the prompt via stdin with `-p -` to avoid this:

```rust
pub struct SpawnableCommand {
    cmd: Command,
    stdin_prompt: Option<String>,  // Written to stdin in background tokio::spawn
}
```

**Mode selection** (`mod.rs:522-526`):
- Default for agent runs: stdin mode (`-p -`)
- Override: `RALPHX_CLAUDE_PROMPT_MODE=arg` forces `-p "<text>"`

### Append System Prompt Mode

**File:** `src-tauri/src/infrastructure/agents/claude/mod.rs:498-609`

Instead of `--agent <name>`, injects behavior via `--append-system-prompt-file`:
1. Resolves agent prompt file from `system_prompt_file` in `ralphx.yaml`
2. Strips YAML frontmatter
3. Passes as `--append-system-prompt-file <path>` (or `--append-system-prompt <content>` fallback)
4. Falls back to native `--agent` if prompt file not found

**Mode selection**: Default is append-system-prompt mode. `RALPHX_USE_NATIVE_AGENT_FLAG=1` forces native `--agent`.

## File Reference

```
src-tauri/src/
├── domain/agents/
│   ├── agentic_client.rs        # AgenticClient trait (interface)
│   ├── types.rs                 # AgentConfig, AgentHandle, AgentRole, etc.
│   ├── capabilities.rs          # ClientCapabilities, ModelInfo
│   └── error.rs                 # AgentError types
├── infrastructure/agents/
│   ├── spawner.rs               # AgenticClientSpawner (state machine bridge)
│   ├── spawner_tests.rs         # Spawn dedup, execution state gating tests
│   └── claude/
│       ├── mod.rs               # Common spawn env, MCP config, name utils, sanitization
│       ├── claude_code_client.rs # CLI spawning (spawn_agent, spawn_agent_streaming)
│       ├── stream_processor.rs  # stream-json parsing & event emission
│       ├── agent_names.rs       # Central agent name constants
│       └── agent_config/
│           └── mod.rs           # YAML config parsing, tool allowlists, settings profiles
├── application/
│   ├── chat_service/
│   │   ├── chat_service_recovery.rs    # Session recovery (replay + respawn)
│   │   └── chat_service_streaming.rs   # Stream processing loop
│   └── session_reopen_service.rs       # Session reopen/reset
└── http_server/
    ├── mod.rs                   # Axum server setup, 50+ routes
    └── handlers/                # Per-endpoint handler implementations

plugins/app/
├── .mcp.json                    # MCP server stdio config
├── agents/*.md                  # 20 agent definitions (frontmatter + prompt)
└── ralphx-mcp-server/src/
    ├── index.ts                 # Main entry, tool dispatch, agent type parsing
    ├── tools.ts                 # 60+ tool definitions + per-agent allowlists
    ├── agentNames.ts            # Agent name constants (TS mirror)
    ├── tauri-client.ts          # HTTP client for Tauri :3847
    ├── plan-tools.ts            # Plan artifact tool schemas
    ├── step-tools.ts            # Step management tool schemas
    ├── worker-context-tools.ts  # Context/artifact tool schemas
    ├── issue-tools.ts           # Review issue tool schemas
    ├── permission-handler.ts    # Permission request two-phase protocol
    └── question-handler.ts      # User question two-phase protocol

config/ralphx.yaml               # Shared runtime config (embedded at compile time)
```
