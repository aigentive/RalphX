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
| Canonical source of truth | RalphX-native delegation rights live in `agents/<agent>/agent.yaml` under `delegation.allowed_targets`. |
| Capability shape stays minimal | Delegation metadata is an allowlist only. Do not add extra knobs unless runtime code actively enforces them. |
| Auto guidance, not prompt drift | If `delegation.allowed_targets` is non-empty, the runtime auto-injects generic delegation system guidance into loaded prompts. Do not hand-copy generic delegation boilerplate into every prompt. |
| Prompts keep workflow specifics only | Prompts may keep role-specific delegation workflow rules (for example bounded reviewer analysis or verifier artifact contracts), but generic policy/authorization belongs in canonical metadata + auto-injection. |
| Backend enforces topology | `delegate_start` must validate caller identity and reject caller→target pairs outside `delegation.allowed_targets`. Prompt text is not the enforcement layer. |
| MCP hides unauthorized delegation tools | Agents without canonical delegation rights must not see `delegate_start` / `delegate_wait` / `delegate_cancel` in the MCP surface. |
| Canonical read-only explorer | Agents that need bounded read-only investigation may delegate to `ralphx-general-explorer` when permitted. |
| Canonical grants stay aligned | Delegating agents need `delegate_start` / `delegate_wait` / `delegate_cancel` in canonical `capabilities.mcp_tools`; runtime injection and MCP authorization derive from that metadata. |
| Caller identity is transport-owned | MCP/server transport injects caller identity for delegation. Models should not be asked to invent or spoof it. |
| Two ledgers, no mirrors | `delegate_start(task_ref)` atomically assigns the immediate caller’s exact task; generic task tools remain private to the delegated session and narrow assignment tools never enumerate the caller ledger. |
| Read-only means filesystem | The general explorer may mutate its private coordination ledger and request assignment settlement; its shell/file-write restrictions remain unchanged. |
| Backend settles authority | Assignment completion requires exact-run intent plus successful termination; failure, cancellation, release, implicit completion, and orphan recovery reopen the exact task. |
| Native Team uses this topology | RX-native Team is a product surface over this provider-neutral delegation contract; removed vendor-specific Team semantics do not return here. |
| Tool naming convention | Prompt prose uses bare tool names like `delegate_start`; config/frontmatter/allowlists use fully qualified MCP names only where that path requires qualification. |
| Waiting is backend-held | Coordinators wait via bounded `delegate_wait` (`wait_timeout_ms`) or `delegate_park` + turn end; never model-side polling loops. |

## Shared-Worktree Coordination

| Rule | Detail |
|---|---|
| One worktree, immediate visibility | Delegates in one coordinated RalphX run see the same worktree immediately. |
| Exclusive writable ownership | Each delegate owns disjoint writable paths, including generated outputs; parallel edits require disjoint source and generated-output ownership. |
| One serialized Rust lane | Cargo, nextest, clippy, coverage, and every Rust build/test invocation are worktree-wide serialized work. |
| One heavyweight validator | Exactly one designated validator owns heavyweight Rust validation and the required single post-Rust-test cleanup; implementation delegates do not duplicate either. |
| Serialized package resources | Package-manager/build commands serialize per package or resource set; the owning validation lane produces committed build output such as MCP `dist` artifacts. |
| Repository rules win | Repository rules remain authoritative for exact commands, cleanup, validation scope, and stricter constraints. |
