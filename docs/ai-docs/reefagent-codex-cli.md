# Reefagent Codex CLI Report

Status: research snapshot on 2026-04-07. This file is the cross-session notebook for how Reefagent integrated Codex CLI and what RalphX can reuse conceptually.

## High-level shape

Reefagent does not bolt Codex directly into one provider-specific codepath. It has a reusable CLI-agent layer plus a Codex-specific provider on top.

Primary files already confirmed:

- `config.example.yaml`
- `docs/providers.md`
- `docs/mcp-provider-layering.md`
- `docs/model-capabilities.md`
- `docs/configuration.md`
- `docs/runtime-artifacts.md`
- `docs/mcp-tools.md`
- `src/providers/codex-cli.ts`
- `src/providers/claude-code.ts`
- `src/providers/cli-agent/process-runner.ts`
- `src/providers/cli-agent/session-manager.ts`
- `src/providers/cli-agent/message-serializer.ts`
- `src/providers/cli-agent/mcp-config.ts`
- `src/core/codex-event-logger.ts`
- `src/core/codex-jsonl.ts`
- `src/registry.ts`
- `src/gateway/http.ts`
- `src/mcp/server.ts`
- `src/mcp/client/client-pool.ts`
- `src/core/config.schema.ts`
- `src/types/config.ts`
- `src/core/provider-session-store.ts`
- `src/agent/run.ts`
- `src/agent/prompt-builder.ts`
- `src/core/model-capabilities.ts`
- `src/providers/codex-cli.test.ts`
- `src/providers/codex-cli.integration.test.ts`
- `src/providers/__snapshots__/codex-cli.test.ts.snap`

## Confirmed design choices

### 1. Separate provider, shared runner

Reefagent keeps a dedicated Codex provider in `src/providers/codex-cli.ts`, but it reuses a shared CLI-agent substrate for:

- process spawning
- session tracking
- inactivity timers
- message serialization
- MCP config shaping

Implication for RalphX:

- a multi-harness abstraction should sit below `ChatService` and above provider-specific spawn/parsing code
- do not fork all Claude chat-service logic into a second near-copy for Codex

### 1a. Prefix-based provider routing

Reefagent resolves providers by model prefix in `src/registry.ts`:

- `pro:` routes to `claude-code`
- `codex:` routes to `codex-cli`

Implication for RalphX:

- harness selection should be explicit and first-class, not inferred only from model naming

### 2. Config-driven execution policy mapping

Reefagent defines Codex-specific execution policies in config:

- `autonomous`
- `supervised`
- `sandboxed`

In `src/providers/codex-cli.ts`, these map to Codex config / sandbox args rather than Claude-style flags. Reefagent also separates initial-run and resume policy args.

Implication for RalphX:

- RalphX should persist logical harness policies, not raw vendor flags
- the harness layer should translate logical policy to Claude or Codex spawn config

Concrete Reefagent details:

- Codex-specific config keys are `executionPolicy` and `reasoningEffort`
- those live in `config.example.yaml`, `src/core/config.schema.ts`, and `src/types/config.ts`
- the current code maps all three Codex policies to `danger-full-access`; the live difference is mostly `approval_policy`

### 3. MCP injection via inline Codex config

Reefagent does not rely on Codex reading a static global config file. It builds repeatable CLI overrides:

- `-c mcp_servers.<name>.url=...`
- `-c mcp_servers.<name>.enabled_tools=[...]`

The bridge is HTTP-based and passes coordination/session metadata through query params.

Implication for RalphX:

- Codex internal MCP should likely be wired by generated per-run config overrides
- this is the strongest alternative to Claude `--plugin-dir`

Concrete Reefagent transport split:

- Claude path: temp stdio MCP config via `src/providers/cli-agent/mcp-config.ts`
- Codex path: HTTP JSON-RPC bridge at `src/gateway/http.ts` on `/mcp/codex`
- both converge on shared builtin tool execution in `src/mcp/server.ts`
- external MCP still routes through `src/mcp/client/client-pool.ts`

### 4. Raw event logging first, normalization second

Reefagent stores raw Codex event JSONL via `src/core/codex-event-logger.ts`, then normalizes assistant messages / tool calls / tool results in `src/core/codex-jsonl.ts`.

Implication for RalphX:

- keep raw provider logs as a permanent audit layer
- build a unified parsed event contract above provider-specific raw streams

### 5. Codex-specific prompt / feature fences

Reefagent injects vendor-aware behavior such as:

- model name mapping from `codex:*`
- reasoning-effort config
- optional structured output schema files
- feature disabling such as `features.multi_agent=false` in coordination cases

Implication for RalphX:

- a Codex harness needs its own capability flags and guardrails
- not every Claude behavior should be forced onto Codex unchanged

Concrete provider differences already confirmed:

- Claude uses `--add-dir ORIGINAL_CWD`; Codex uses `-C ORIGINAL_CWD`
- Claude sends prompt on `stdin`; Codex passes the prompt after `--`
- Claude uses wrapped MCP tool ids; Codex uses logical names in `enabled_tools`
- Claude strips `CLAUDECODE` from env; Codex has no matching scrub
- Claude and Codex both declare `managesOwnSessions = true`

## Reefagent config examples already found

From `config.example.yaml`:

- `codex.model: codex:gpt-5.4`
- `codex.reasoningEffort: xhigh`
- `codex.executionPolicy: supervised`
- `codex-agent.executionPolicy: autonomous`
- `codex-mini-agent.model: codex:codex-mini`

RalphX now uses `gpt-5.5` as its own primary Codex default; the Reefagent value above is retained only as the historical source snapshot.

This is the closest existing template for the RalphX defaults you requested.

## Side-by-side provider matrix

### Spawn surface

- Claude: `-p`, `--output-format json|stream-json`, `--dangerously-skip-permissions`, `--tools ""`, `--mcp-config`, `--strict-mcp-config`, `--allowedTools`, `--append-system-prompt`, `--resume`
- Codex: `codex exec`, `--json`, `-C` or `--skip-git-repo-check`, `-m`, repeatable `-c`, `model_reasoning_effort`, `approval_policy`, `sandbox_mode`, `resume`

### Prompt transport

- Claude: prompt over `stdin`
- Codex: prompt as trailing CLI argument after `--`

### Tool naming

- Claude: wrapped ids like `mcp__reefagent__exec`
- Codex: logical names like `exec` or `jira__get_myself`

### MCP transport

- Claude: stdio subprocess
- Codex: HTTP JSON-RPC bridge

### Stream parsing

- Claude: native provider parser in `src/providers/claude-code.ts`
- Codex: normalized through `src/core/codex-jsonl.ts`

### Session persistence

- both use shared provider-session storage
- both are delegated-session providers
- transcripts persist only a thin session shadow while the CLI provider owns full context

## Raw event handling already found

Confirmed event handling surface:

- `run_started`
- `stdout_event`
- `run_finished`
- nested Codex `thread.started`, `turn.started`, `item.started`, `item.completed`, `item.updated`, `turn.completed`
- nested item types include `agent_message`, `mcp_tool_call`, `command_execution`, `error`, and collaboration items in the delegation flows

## Reusable ideas for RalphX

### Keep logical state above vendor state

Reefagent treats Codex session ids and raw events as provider details, not the primary app contract.

RalphX equivalent:

- `AgentHarness = Claude | Codex`
- shared `AgentRun`
- shared normalized events
- provider-specific raw payloads stored alongside the shared model

### Generate MCP/tool grants per run

Reefagent computes the effective Codex MCP allowlist from the requested logical tools at spawn time.

RalphX equivalent:

- keep one logical tool grant model
- translate it to Claude frontmatter / YAML / MCP allowlists and to Codex `mcp_servers.*.enabled_tools`

### Separate model selection from harness selection

Reefagent separates provider model strings from the rest of the agent identity.

RalphX equivalent:

- settings must let the user combine harness + model + effort per lane
- example: Codex + `gpt-5.5` + `xhigh` for ideation, Codex + `gpt-5.4-mini` + `medium` for verification, Claude for execution

### Share prompt construction, not provider internals

Reefagent assembles prompts before provider handoff in `src/agent/run.ts` and `src/agent/prompt-builder.ts`.

RalphX equivalent:

- prompt construction should be provider-neutral
- Claude/Codex-specific transport and parsing should sit below that layer

## Immediate RalphX relevance

Reefagent already demonstrates:

- Codex as a first-class CLI harness
- vendor-native MCP bridging without plugin-dir
- Codex raw-event capture
- normalized event extraction over raw JSONL
- per-run config injection instead of mutating global vendor files

That makes Reefagent the best local reference for RalphX phase 1.

## Known gaps or inconsistencies in Reefagent

- `docs/providers.md` still shows a Codex `enabled_tools` example using wrapped MCP names, while the code/tests use logical names
- Codex config comments imply tighter sandbox differences than the current code actually enforces
- Claude install hints are not perfectly consistent across docs and code

These are useful warnings for RalphX:

- keep the logical contract in one place
- generate vendor-specific surfaces from that contract
- verify docs/examples against live spawn code and tests
