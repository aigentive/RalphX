# Harness-Specific Agent Config And Prompt Architecture

## Goal

Replace the current Claude-shaped agent prompt/config source of truth with a canonical RalphX agent definition layer that can produce harness-specific outputs for Claude, Codex, and future harnesses.

## Status

In progress.

Landed so far:
- phase 1 pilot skeleton under `agents/` for `ralphx-ideation` and `ralphx-utility-session-namer`
- model-native Verify Plan now reuses the active Plan conversation model and its general-purpose delegation surface; the former verifier cohort was removed
- specialist/debate cohort canonicalized as Claude-only agents: ideation specialists plus `ralphx-ideation-advocate` / `ralphx-ideation-critic`
- first cross-harness execution pair canonicalized: `ralphx-execution-reviewer` + `ralphx-execution-merger`
- resolver-backed canonical prompt loading for migrated agents on the Codex path
- Claude runtime now materializes a generated plugin cache dir instead of reading authored prompt files directly from `plugins/app/agents`

Tracker reference:
- [AGENTS.md](/Users/example/Code/ralphx/AGENTS.md)

## Problem

Today the system mixes three concerns into one Claude-oriented source:

1. Canonical RalphX agent identity and policy
2. Claude plugin/frontmatter/runtime bootstrap details
3. Prompt body text

That coupling is now incorrect because Codex is using the same prompt bodies and some of those prompt bodies contain Claude-only semantics such as:
- `Task(...)`
- Claude frontmatter/tool assumptions
- `mcpServers` guidance
- Claude-specific subagent behavior notes

Codex currently wraps those same prompts via `compose_codex_prompt(...)`, which means Codex inherits prompt content that was authored for Claude plugin execution rather than for Codex-native runtime behavior.

## Current State

### Source Of Truth Today

- Runtime agent config is loaded from [ralphx.yaml](/Users/example/Code/ralphx/config/ralphx.yaml)
- Canonical prompt bodies now live under [agents](/Users/example/Code/ralphx/agents)
- Claude runtime resolves `system_prompt_file` from [agent_config/mod.rs](/Users/example/Code/ralphx/src-tauri/src/infrastructure/agents/claude/agent_config/mod.rs)
- Claude runtime/plugin bootstrap is built in [claude/mod.rs](/Users/example/Code/ralphx/src-tauri/src/infrastructure/agents/claude/mod.rs)
- Codex currently loads those same prompt files through [compose_codex_prompt](/Users/example/Code/ralphx/src-tauri/src/infrastructure/agents/codex/mod.rs)
- Chat/runtime Codex spawn paths call that from [chat_service_context.rs](/Users/example/Code/ralphx/src-tauri/src/application/chat_service/chat_service_context.rs)

### Why This Is Wrong

- Claude plugin markdown is being treated as universal prompt source
- Claude-only prompt syntax leaks into Codex
- Claude frontmatter is mixed with prompt content
- Harness-specific spawn semantics are not isolated
- Adding Codex-specific behavior by branching inside shared prompts will make prompts larger, harder to maintain, and less testable

## Design Principles

1. RalphX canonical config must be the source of truth.
2. Claude plugin assets must become generated outputs, not the universal source.
3. Prompt bodies must be harness-specific where semantics differ.
4. Shared policy must stay shared to avoid duplicated grants and drift.
5. Shared prompt text is allowed only through an explicit harness-neutral prompt layer, never by silently reusing another harness prompt.
6. Harness-specific runtime bootstrap details must not leak into the wrong harness.
7. Migration must be incremental and agent-by-agent, not a big-bang rewrite.

## Target Architecture

## Canonical Layout

Recommended initial structure:

```text
agents/
  <agent-name>/
    agent.yaml
    shared/
      prompt.md
    claude/
      agent.yaml
      prompt.md
    codex/
      agent.yaml
      prompt.md
```

The important distinction is:
- `shared/prompt.md` means “this text is intentionally harness-neutral and safe for every enabled harness”
- `claude/prompt.md` or `codex/prompt.md` means “this harness needs its own prompt body”

Missing harness-specific prompt must never mean “fall back to some other harness prompt”.

## Shared Versus Harness-Specific Data

### Shared

Belongs in `agent.yaml`:

- canonical agent id/name
- role classification
- lane binding intent
- shared model/effort policy defaults where truly harness-neutral
- settings profile hooks
- whether the agent is helper-only, orchestrator, team lead, specialist, critic, worker, reviewer, merger, etc.
- generation flags needed to produce harness artifacts

Belongs in `shared/prompt.md` only when truly harness-neutral:

- role framing that is identical across harnesses
- workflow guidance that does not mention harness-specific delegation/runtime/tool semantics
- user-facing quality bar and output structure
- repo-specific constraints that are equally valid on every supported harness

### Harness-Specific

Belongs in `<harness>/agent.yaml` when needed:

- harness-only metadata such as `disallowed_tools`
- harness-only skills
- harness-only generation hints like Claude `max_turns`
- future harness-specific runtime metadata that is not universally meaningful

Belongs in harness-specific prompt/config:

- prompt body
- harness-specific delegation syntax
- Claude frontmatter
- Claude `Task(...)` instructions
- Claude `mcpServers` guidance
- Codex-native delegation or subagent instructions
- harness-specific bootstrap framing
- harness-specific tool-calling notes

## Prompt Resolution Rules

Resolution must be explicit and deterministic:

1. Load `agent.yaml`
2. Load optional `<harness>/agent.yaml` shallow overlay for that harness
3. If `<agent>/<harness>/prompt.md` exists, use it
4. Else if `shared/prompt.md` exists, use it
5. Else if the agent is canonical but has no prompt for that harness, treat it as unsupported on that harness
6. Only non-canonical legacy agents may fall back to the old Claude plugin prompt source during migration

Non-negotiable:
- never use `claude/prompt.md` as an implicit fallback for Codex
- never use `codex/prompt.md` as an implicit fallback for Claude
- unsupported harness roles should fail closed or stay unavailable, not silently inherit the wrong semantics

## Runtime Model

### Canonical Resolver

Add a RalphX agent resolver service that loads:

1. canonical shared agent definition
2. harness-specific prompt body
3. harness-specific runtime metadata

The resolver must return a harness-ready agent definition for:
- Claude backend spawns
- Codex backend spawns
- helper agents
- team/specialist/critic agent paths

### Claude Path

Claude still requires plugin-discoverable agent assets.

Therefore:
- canonical config remains source of truth
- Claude plugin markdown/frontmatter becomes generated output
- generated assets feed the existing Claude plugin/runtime path through an injected generated plugin cache dir, not the authored repo plugin tree

This keeps Claude compatible with its plugin agent discovery and subagent spawning model.

### Claude Generated Plugin Cache

Claude still expects a plugin root containing:

- `agents/`
- `.mcp.json`
- `ralphx-mcp-server/`
- other plugin runtime assets

So the generated output is not just prompt files. RalphX should materialize a managed Claude plugin cache dir that:

- reuses the base plugin runtime assets (`.mcp.json`, MCP server, hooks, skills, plugin manifest, etc.)
- overlays generated `agents/*.md` files for canonical agents
- leaves non-canonical agents as legacy fallback links during migration

Recommended locations:

- development: repo-local `.artifacts/generated/claude-plugin/`
- production: app-support `generated/claude-plugin/`

Do not rely on ephemeral `/tmp` as the long-lived generated plugin root.

### Codex Path

Codex should stop reading Claude plugin prompt files as the canonical prompt body.

Instead:
- Codex loads the harness-specific Codex prompt body from canonical config
- Codex prompt composition no longer depends on Claude frontmatter or Claude-oriented prompt text
- Codex-specific delegation semantics become first-class instead of patched into shared prompts

## Generation Pipeline

### Required Output For Claude

Generate Claude assets from canonical config:

- plugin agent markdown
- frontmatter
- tool lists
- `mcpServers` declarations where needed
- default output path `agents/<agent-name>.md`; do not add per-agent output-path config until a real override case exists

These outputs should be written into the generated Claude plugin cache dir. `plugins/app/agents/` is no longer an authored-source fallback.

### Required Output For Codex

Codex does not need Claude plugin frontmatter.

Codex should consume:
- resolved prompt text
- resolved MCP/tool policy
- resolved runtime model/effort/sandbox/approval settings

from the canonical resolver directly.

## Migration Plan

## Phase 0: Freeze Scope

Before behavior changes:
- document the current architecture
- define canonical schema
- define generated Claude outputs
- define Codex resolver interface

## Phase 1: Canonical Config Skeleton

Implement:
- `agents/<agent>/agent.yaml`
- optional `agents/<agent>/<harness>/agent.yaml`
- optional `agents/<agent>/shared/prompt.md`
- optional `agents/<agent>/claude/prompt.md`
- optional `agents/<agent>/codex/prompt.md`
- resolver that can load one agent from the new structure

No runtime cutover yet.

## Phase 2: Ideation Pilot

Migrate only:
- `ralphx-ideation`
- `ralphx-utility-session-namer`

Reason:
- they are the highest-value agents for current Codex/Claude divergence
- they directly affect the current product issues

Deliverables:
- Codex uses Codex-native prompt files for these agents
- Claude plugin assets for these agents are generated from canonical config
- no Claude-only instructions remain in the Codex versions

## Phase 3: Core Execution Agents

Migrate:
- `reviewer`
- `merger`
- `worker`
- `coder`
- `review-chat`
- `ralphx-chat-task`
- `ralphx-chat-project`

Start with the smaller cross-harness pilot:
- `ralphx-execution-reviewer`
- `ralphx-execution-merger`

Requirements for that pair:
- preserve Claude prompt bodies exactly through canonical `claude/prompt.md`
- author real `codex/prompt.md` files instead of inheriting Claude wording
- Codex prompt tests must reject Claude-only syntax such as:
  - `Task(`
  - `mcpServers`
  - `CLAUDE_PLUGIN_ROOT`
  - Claude-specific subagent failure caveats that are no longer true on Codex
- runtime must prefer canonical Codex prompts once the pair is migrated

Status:
- landed
- the next execution/user-facing cohort (`ralphx-execution-worker`, `ralphx-execution-coder`, `ralphx-review-chat`) is also landed

Requirements that were enforced for the execution/user-facing cohort:
- preserve existing Claude prompt bodies exactly through canonical `claude/prompt.md`
- author real Codex prompt bodies for:
  - `ralphx-execution-worker`
  - `ralphx-execution-coder`
  - `ralphx-review-chat`
- Codex prompt tests must reject Claude-only syntax and team/task registry assumptions
- worker/coder Codex prompts should preserve the execution/re-execution/state/validation contract without assuming Claude `Task(...)` helpers

Status:
- the low-divergence chat assistant cohort (`ralphx-chat-task`, `ralphx-chat-project`) is landed as shared-prompt canonical agents

Shared-prompt agents:
- `ralphx-chat-task`
- `ralphx-chat-project`

Requirements for the chat assistant cohort:
- preserve existing Claude prompt bodies exactly through canonical `shared/prompt.md`
- both Claude and Codex should resolve the same shared prompt body unless a real harness divergence appears later
- Codex prompt tests must still reject Claude-only syntax

Next support/specialist candidates:
- `review-history`
- remaining support/chat helpers that still read legacy plugin-authored prompts

Shared-prompt support cohort:
- `ralphx-review-history`
- `ralphx-project-analyzer`
- `ralphx-memory-capture`
- `ralphx-memory-maintainer`

Requirements for the support cohort:
- preserve existing Claude prompt bodies exactly through canonical `shared/prompt.md`
- keep both harnesses on the same prompt body unless a real harness divergence appears
- Codex prompt tests must still reject Claude-only syntax

Shared-prompt general cohort:
- `ralphx-research-deep-researcher`
- `ralphx-execution-orchestrator`
- `ralphx-qa-prep`
- `ralphx-qa-executor`

Requirements for the general cohort:
- preserve existing Claude prompt bodies exactly through canonical `shared/prompt.md`
- keep both harnesses on the same prompt body unless a real harness divergence appears
- Codex prompt tests must still reject Claude-only syntax

Status:
- landed
- the only remaining live legacy runtime prompt is `ralphx-ideation-readonly`

Final live runtime prompt:
- `ralphx-ideation-readonly`

Requirements for the readonly ideation agent:
- preserve the existing Claude prompt body exactly through canonical `claude/prompt.md`
- keep Claude-only readonly metadata in `claude/agent.yaml`
- author a Codex-native readonly ideation prompt that preserves accepted-session / child-session semantics without `Task(...)` or other Claude-only orchestration syntax

## Phase 4: Specialists And Critics

Migrate:
- ideation specialists
- advocate/critic

This is where harness-specific delegation semantics matter most.

## Phase 5: Remove Legacy Source-Of-Truth Status

Landed:
- `plugins/app/agents/*.md` is no longer treated as authored source
- the deprecated markdown files under `plugins/app/agents/` have been deleted
- `config/ralphx.yaml` now points live runtime agents at canonical prompt paths under `agents/`
- canonical-tree and generated-artifact tests no longer depend on reading legacy prompt markdown

Remaining:
- make canonical `agents/` resolution mandatory for all live runtime agents in every runtime path
- remove any remaining live-agent prompt fallback assumptions from the generated Claude plugin/runtime path
- rewrite docs/comments that still describe plugin markdown as source-of-truth

## Agent Cohorts To Migrate

### Pilot

- `ralphx-ideation`
- `ralphx-utility-session-namer`

### Core User-Facing

- `ralphx-chat-task`
- `ralphx-chat-project`
- `review-chat`
- `review-history`

### Execution Pipeline

- `ralphx-execution-worker`
- `ralphx-execution-coder`
- `ralphx-execution-reviewer`
- `ralphx-execution-merger`

### Debate

- `ralphx-ideation-advocate`
- `ralphx-ideation-critic`

### Specialists

- `ralphx-ideation-specialist-backend`
- `ralphx-ideation-specialist-frontend`
- `ralphx-ideation-specialist-infra`

## Required Tests

### Resolver Tests

- canonical shared config loads
- harness-specific prompt file loads
- harness-specific override resolution works
- unknown/missing harness prompt fails clearly
- all live runtime agent ids resolve through canonical `agents/` first with no silent fallback to plugin-authored prompt files

### Claude Generation Tests

- generated Claude markdown/frontmatter matches expected fields
- generated tool grants match canonical MCP policy
- `mcpServers` generation is correct
- existing Claude runtime can still discover and spawn generated agents
- generated Claude prompt body and authored Claude metadata stay semantically aligned with the legacy source where that legacy frontmatter was the authored truth
- generated Claude `tools` and `mcpServers` stay aligned with runtime config (`config/ralphx.yaml`) even when legacy plugin frontmatter had drifted stale
- explicit Claude-only compatibility aliases (for example `worker-team`) stay encoded in `claude/agent.yaml`, not in ad hoc generator special cases

### Codex Prompt Tests

- Codex prompt bodies contain no Claude-only frontmatter
- Codex prompt bodies contain no `Task(...)` unless explicitly intended
- Codex prompt composition still preserves RalphX bootstrap wrappers where needed

### Runtime Contract Tests

- Claude path still resolves the expected prompt/tool/runtime config
- Codex path resolves the Codex-specific prompt/tool/runtime config
- same agent id can resolve different harness prompt bodies from the same shared definition

### Integration Tests

- ideation pilot agents spawn correctly on both harnesses
- Claude subagent behavior remains intact
- Codex runtime no longer consumes Claude plugin prompt bodies for migrated agents

## Acceptance Criteria

The migration is successful when:

1. Canonical RalphX config is the source of truth for migrated agents.
2. Claude plugin markdown/frontmatter is generated for migrated agents.
3. Codex no longer loads Claude plugin prompt bodies for migrated agents.
4. Codex prompts for migrated agents contain no Claude-only semantics unless explicitly preserved by design.
5. MCP grants remain aligned between canonical config, generated Claude assets, and runtime allowlists.
6. Existing Claude plugin-based subagent behavior still works.
7. The ideation pilot works on both Claude and Codex without prompt-body leakage.

## Non-Goals

- full multi-harness subagent parity in one step
- removing Claude plugin discovery immediately
- redesigning all agent instructions during this migration
- changing lane policy/model policy unless needed for harness split

## Open Questions

1. Codex-native delegation/subagent model
Do we map RalphX specialists/critics to Codex-native subagents, Codex tools, or keep some delegation in RalphX MCP/backend instead?

2. Generated asset cache policy
Do we want a stronger cache lifecycle/versioning policy for generated Claude plugin assets beyond the current generated runtime directory?

3. Shared versus harness-specific model defaults
Should model/effort defaults remain shared unless overridden, or should they move into harness sections early?

4. Agent frontmatter ownership
How much Claude frontmatter should be generated from canonical schema versus templated as raw harness-specific text?

## Recommended First Implementation Slice

Implement only:

- canonical config skeleton
- resolver service
- Claude generation for 3 agents
- Codex-native prompt loading for 3 agents
- tests for those 3 agents

Pilot agents:
- `ralphx-ideation`
- `ralphx-utility-session-namer`

That is the smallest slice that fixes the current Codex/Claude prompt coupling without forcing a repo-wide migration in one round.
