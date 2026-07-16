---
paths:
  - "agents/**"
  - "src-tauri/src/infrastructure/agents/**"
  - "src-tauri/src/http_server/**"
  - "plugins/app/ralphx-mcp-server/src/**"
  - "config/**"
  - "AGENTS.md"
  - "CLAUDE.md"
---

# Delegation Topology Rules

**Required Context:** `agent-mcp-tools.md` | `multi-harness.md`

| Rule | Detail |
|---|---|
| Canonical source of truth | Non-team RalphX-native delegation rights live in `agents/<agent>/agent.yaml` under `delegation.allowed_targets`. |
| Capability shape stays minimal | Delegation metadata is an allowlist only. Do not add extra knobs unless runtime code actively enforces them. |
| Auto guidance, not prompt drift | If `delegation.allowed_targets` is non-empty, the runtime auto-injects generic delegation system guidance into loaded prompts. Do not hand-copy generic delegation boilerplate into every prompt. |
| Prompts keep workflow specifics only | Prompts may keep role-specific delegation workflow rules (for example bounded reviewer analysis or verifier artifact contracts), but generic policy/authorization belongs in canonical metadata + auto-injection. |
| Backend enforces topology | `delegate_start` must validate caller identity and reject caller→target pairs outside `delegation.allowed_targets`. Prompt text is not the enforcement layer. |
| MCP hides unauthorized delegation tools | Agents without canonical delegation rights must not see `delegate_start` / `delegate_wait` / `delegate_cancel` in the MCP surface. |
| Canonical read-only explorer | Non-team agents that need bounded read-only investigation may delegate to `ralphx-general-explorer` when permitted. |
| Canonical grants stay aligned | Delegating agents need `delegate_start` / `delegate_wait` / `delegate_cancel` in canonical `capabilities.mcp_tools`; runtime injection and MCP authorization derive from that metadata. |
| Caller identity is transport-owned | MCP/server transport injects caller identity for delegation. Models should not be asked to invent or spoof it. |
| Team semantics stay separate | Do not retrofit Claude-only Team semantics into this rule. This topology governs RalphX-native non-team delegation only. |
| Tool naming convention | Prompt prose uses bare tool names like `delegate_start`; config/frontmatter/allowlists use fully qualified MCP names only where that path requires qualification. |
