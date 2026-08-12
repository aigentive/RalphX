# RalphX Runtime Config Management System

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

`config/ralphx.yaml` is the shared runtime compatibility config for agent definitions, prompt/tool wiring, and Claude/default runtime settings that have not yet been fully extracted into `agents/` and `config/harnesses/*`. The app-level harness selection layer is provider-neutral; Codex-specific runtime behavior is layered on top of these shared agent/lane definitions rather than replacing them.

---

## Schema Overview

```yaml
tool_sets:          # Named reusable CLI tool groups
  base_tools: [Read, Grep, Glob, Bash, WebFetch, WebSearch, Skill]

claude:             # Global Claude CLI runtime settings
  mcp_server_name: ralphx
  permission_mode: default
  settings_profile: default
  settings_profiles:
    default: { sandbox: { enabled: false } }
    z_ai: { extends: default, env: { ... } }

agents:             # All agent definitions (20 currently)
  - name: ralphx-execution-worker
    system_prompt_file: agents/ralphx-execution-worker/claude/prompt.md
    model: sonnet
    tools: { extends: base_tools, include: [Write, Edit, Task] }
    mcp_tools: [start_step, complete_step, ...]
    preapproved_cli_tools: [Read, Write, Edit, ...]
```

---

## Rust Deserialization Structs

**File:** `src-tauri/src/infrastructure/agents/claude/agent_config/mod.rs`

### Root Config

```rust
struct RalphxConfig {
    tool_sets: HashMap<String, Vec<String>>,   // Named CLI tool groups
    claude: ClaudeRuntimeConfigRaw,             // Global runtime settings
    agents: Vec<AgentConfigRaw>,                // Agent definitions
}
```

### Agent Definition

```rust
struct AgentConfigRaw {
    name: String,                              // Short name (no prefix)
    tools: AgentToolsSpec,                     // CLI tool strategy
    mcp_tools: Vec<String>,                    // MCP tool names (bare)
    preapproved_cli_tools: Vec<String>,        // Extra pre-approved variants
    system_prompt_file: String,                // Path to .md prompt file
    model: Option<String>,                     // Model alias (haiku|sonnet|opus)
    settings_profile: Option<String>,          // Per-agent profile override
}
```

### Tool Specification

```rust
struct AgentToolsSpec {
    mcp_only: bool,         // true → no CLI tools (--tools "")
    extends: Option<String>, // Base tool set name from tool_sets
    include: Vec<String>,    // Additional tools appended after inherited
}
```

**Resolution:** `extends` tools + `include` tools, deduplicated via `HashSet` (preserves first-seen order).

### Claude Runtime Config

```rust
struct ClaudeRuntimeConfigRaw {
    mcp_server_name: String,                   // MCP server ID → tool prefix
    setting_sources: Option<Vec<String>>,       // --setting-sources: [user, project, local]
    permission_mode: String,                    // --permission-mode
    dangerously_skip_permissions: bool,         // --dangerously-skip-permissions
    permission_prompt_tool: String,             // MCP tool for permission prompts
    append_system_prompt_file: bool,            // --append-system-prompt-file vs inline
    settings_profile: Option<String>,           // Selected profile name
    settings_profile_defaults: Option<Value>,   // Merged into ALL profiles
    settings_profiles: HashMap<String, Value>,  // Named --settings JSON payloads
    settings: Option<Value>,                    // Legacy --settings (backward compat)
}
```

---

## Configuration Loading Pipeline

### Load Path Resolution

| Priority | Source | When Used |
|----------|--------|-----------|
| 1 | `RALPHX_CONFIG_PATH` env var | Custom config path |
| 2 | `<project-root>/config/ralphx.yaml` | Normal operation |
| 3 | `EMBEDDED_CONFIG` (compiled in) | File missing/unreadable |

### Processing Flow

```
config/ralphx.yaml (or EMBEDDED_CONFIG)
    │
    ▼
serde_yaml::from_str() → RalphxConfig
    │
    ├── For each agent:
    │   ├── resolve_tools(raw, tool_sets) → resolved_cli_tools
    │   └── resolve_claude_settings(raw, profile) → settings JSON
    │
    └── Build LoadedConfig { agents: Vec<AgentConfig>, claude: ClaudeRuntimeConfig }
         │
         ▼
    OnceLock<LoadedConfig> — cached for entire process lifetime
```

### Public API Functions

| Function | Returns | Used By |
|----------|---------|---------|
| `agent_configs()` | All agent configs | Diagnostics |
| `claude_runtime_config()` | Global claude settings | CLI builder |
| `get_agent_config(name)` | Single agent config | Spawn-time lookup |
| `get_effective_settings(name)` | Merged settings JSON | --settings flag |
| `get_allowed_tools(name)` | CLI tools string | --tools flag |
| `get_preapproved_tools(name)` | Allowlist string | --allowedTools flag |

---

## Settings Profile System

### Profile Resolution Hierarchy

```
1. Per-agent env var: RALPHX_CLAUDE_SETTINGS_PROFILE_<AGENT_NAME>
2. Per-agent YAML: agent.settings_profile
3. Global env var: RALPHX_CLAUDE_SETTINGS_PROFILE
4. Global YAML: claude.settings_profile
5. Fallback: "default" profile (if exists)
6. Legacy: claude.settings (backward compat)
```

### Profile Extension

Profiles can extend one or more base profiles:

```yaml
settings_profiles:
  default:
    sandbox: { enabled: false }
  z_ai:
    extends: default                    # Single parent
    env: { ANTHROPIC_BASE_URL: "..." }
  custom:
    extends: [default, z_ai]           # Multiple parents (left-to-right merge)
```

- Circular extends are detected and logged as warning
- Extension is resolved recursively before merging
- Child profile values override parent values (deep merge)

### Profile Defaults

`settings_profile_defaults` is merged into every selected profile:

```yaml
claude:
  settings_profile_defaults:
    permissions:
      deny:
        - Read(./.env)
        - Read(./.env.*)
```

This ensures `.env` protection applies to all profiles regardless of selection.

---

## Environment Variable Override System

### Three Levels of Overrides

| Level | Env Var Pattern | Example | Scope |
|-------|----------------|---------|-------|
| Global profile | `RALPHX_CLAUDE_SETTINGS_PROFILE` | `=z_ai` | All agents |
| Per-agent profile | `RALPHX_CLAUDE_SETTINGS_PROFILE_<NAME>` | `_ORCHESTRATOR_IDEATION=default` | Single agent |
| Profile field | `RALPHX_<KEY>` | `RALPHX_ANTHROPIC_BASE_URL=...` | Overrides `env.<KEY>` in selected profile |

**Agent name normalization:** Non-alphanumeric chars → `_`, uppercased.
- `ralphx-ideation` → `ORCHESTRATOR_IDEATION`
- `ralphx-execution-worker` → `RALPHX_WORKER`

### Profile Field Override Example

```yaml
# In config/ralphx.yaml:
settings_profiles:
  z_ai:
    env:
      ANTHROPIC_AUTH_TOKEN: your_zai_api_key    # Default value
      ANTHROPIC_BASE_URL: https://api.z.ai/...  # Default value
```

```bash
# In src-tauri/.env (override at runtime):
RALPHX_ANTHROPIC_AUTH_TOKEN=real_production_key
RALPHX_ANTHROPIC_BASE_URL=https://custom.endpoint/v1
```

The `apply_prefixed_env_overrides()` function scans `settings.env.*` keys and replaces values with `RALPHX_<KEY>` environment variables.

---

## Tool Allowlist Generation

### CLI Tools (--tools flag)

```
get_allowed_tools(agent_name) →
    if mcp_only: ""  (empty string = no CLI tools)
    else: resolved_cli_tools.join(",")

resolved_cli_tools = tool_sets[extends] ∪ include (deduplicated)
```

**Example:** `ralphx-execution-worker` with `extends: base_tools, include: [Write, Edit, Task]`
→ `Read,Grep,Glob,Bash,WebFetch,WebSearch,Skill,Write,Edit,Task`

### Preapproved Tools (--allowedTools flag)

```
get_preapproved_tools(agent_name) →
    MCP tools:       mcp__<server>__<tool> for each mcp_tools entry
  + CLI tools:       all resolved_cli_tools (unless mcp_only)
  + Preapproved:     preapproved_cli_tools entries (e.g., Task(Plan))
  + Memory skills:   Skill(ralphx:rule-manager) etc. (memory agents only)
```

**Example:** `ralphx-execution-worker` generates ~35 preapproved tools:
- `mcp__ralphx__start_step`, `mcp__ralphx__complete_step`, ...
- `Read`, `Write`, `Edit`, `Bash`, `Task`, ...
- `Task(Plan)`

### MCP-Only Agents

`ralphx-utility-session-namer` uses `mcp_only: true`:
- `--tools ""` → no CLI tools available
- Only MCP tools via `--allowedTools`

---

## Agent Spawn Integration

### Spawn-Time Command Construction

```
build_spawnable_command()
    │
    ├── build_base_cli_command(cli_path, plugin_dir, agent_name)
    │   ├── --plugin-dir <plugin_dir>
    │   ├── --agent <agent_name>
    │   ├── --permission-mode <from claude config>
    │   ├── --setting-sources <from claude config>
    │   ├── --mcp-config <temp file with agent-type scoping>
    │   ├── --permission-prompt-tool <normalized MCP tool>
    │   ├── --settings <resolved profile JSON>
    │   └── [optional: --dangerously-skip-permissions]
    │
    ├── add_prompt_args(cmd, plugin_dir, prompt, agent_name, resume)
    │   ├── --tools <get_allowed_tools(agent_name)>
    │   ├── --allowedTools <get_preapproved_tools(agent_name)>
    │   ├── --model <from agent config>
    │   ├── --append-system-prompt-file <system_prompt_file>
    │   │   OR --append-system-prompt <inline content>
    │   └── -p <prompt> OR --resume <session_id>
    │
    └── configure_spawn(cmd, working_dir, has_stdin)
        ├── Set current_dir (worktree-aware)
        └── Configure stdio pipes
```

### Dynamic MCP Config

Each spawn generates a temporary MCP config at `/tmp/ralphx-mcp-<pid>-<uuid>.json`:

```json
{
  "mcpServers": {
    "ralphx": {
      "command": "node",
      "args": ["<mcp-server-path>/dist/index.js"],
      "env": {
        "RALPHX_AGENT_TYPE": "ralphx-execution-worker",
        "RALPHX_MCP_URL": "http://localhost:3847"
      }
    }
  }
}
```

The `RALPHX_AGENT_TYPE` env var enables server-side tool filtering in `tools.ts`.

---

## Configuration Validation & Caching

| Aspect | Behavior |
|--------|----------|
| **Caching** | `OnceLock<LoadedConfig>` — loaded once, never reloaded |
| **Missing file** | Falls back to `EMBEDDED_CONFIG` (compiled at build time) |
| **Invalid YAML** | Returns error, blocks agent spawn |
| **Unknown fields** | Silently ignored by serde (`deny_unknown_fields` not used) |
| **Missing agent** | `get_agent_config()` returns `None`, spawn fails gracefully |
| **Circular extends** | Detected, logged as warning, stops recursion |
| **Tool dedup** | `HashSet` ensures no duplicate tool entries |

---

## Key Files

| File | Purpose |
|------|---------|
| `config/ralphx.yaml` | Shared runtime compatibility config |
| `src-tauri/src/infrastructure/agents/claude/agent_config/mod.rs` | YAML parsing, config resolution, tool allowlist generation |
| `src-tauri/src/infrastructure/agents/claude/mod.rs` | CLI command building, MCP config generation |
| `src-tauri/src/application/agents/spawner.rs` | State machine → agent spawn orchestration |
| `src-tauri/src/domain/agents/types.rs` | Domain agent types (AgentConfig, AgentRole) |
| `plugins/app/ralphx-mcp-server/src/tools.ts` | Server-side MCP tool registry composition and filtering facade |
| `agents/*/agent.yaml` + prompt files | Canonical agent definitions and prompt bodies |
