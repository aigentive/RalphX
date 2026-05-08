---
paths:
  - "src-tauri/src/application/**"
  - "src-tauri/src/infrastructure/agents/**"
  - "src-tauri/src/commands/**"
  - "frontend/src/**"
  - "docs/**"
  - "AGENTS.md"
---

# Multi-Harness Rules

**Required Context:** `agent-mcp-tools.md` | `task-execution-agents.md`

| Rule | Detail |
|---|---|
| Prefer harness-neutral boundaries | New runtime/config/event work should prefer registries, descriptors, or factories keyed by `AgentHarnessKind` over ad hoc `claude|codex` branching. |
| Preserve legacy data | Provider-neutral fields must stay additive/derivable from legacy Claude-only fields until an explicit migration/removal step lands. |
| Treat capabilities as harness-specific | Team mode, stream protocol, sandbox flags, approvals, and MCP/plugin behavior must be capability-driven, not assumed universal. |
| Claude-only features stay explicit | If a feature is currently Claude-only, keep that limitation explicit in code and docs; do not silently imply Codex parity. |
| Codex defaults stay Codex-shaped | When a lane resolves to Codex, never leak Claude model names, effort labels, or plugin assumptions into the effective runtime config. |
| Startup/runtime wiring must stay centralized | New harness discovery/bootstrap logic belongs in shared runtime registries/adapters, not scattered across app-layer callsites. |
| Prefer provider-neutral concrete names | Shared app/runtime types, builders, and comments should prefer provider-neutral names like `AppChatService`; keep Claude-named concrete aliases only where compatibility still requires them. |
| New harnesses extend the shared surface | Adding another harness must start from the shared registries/bundles/adapters and docs, not from a new pairwise `claude+X` special case. |
| Settings/docs must stay in sync | Any user-visible harness capability, limitation, or lane-setting change must update user docs and the relevant `.claude/rules` file in the same PR. |
| Providers own readiness/defaults | Provider settings own CLI readiness, enablement, default-provider selection, provider defaults, and first-enable lane application; lane settings only choose among enabled/validated providers. |
| Codex MCP launch modes are locked | Until RalphX MCP supports weaker Codex modes, Codex approval/sandbox defaults and settings must remain `never` / `danger-full-access`. |
