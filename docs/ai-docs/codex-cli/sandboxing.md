# Codex Sandboxing

Official docs: `https://developers.openai.com/codex/concepts/sandboxing`

Snapshot notes:

- Codex has a native sandbox model and does not rely on Claude plugin-dir semantics.
- Official docs distinguish `read-only`, `workspace-write`, and `danger-full-access` style modes.
- Sandbox policy is intertwined with approvals, writable roots, and network access.

RalphX notes:

- Codex sandbox semantics are the biggest vendor-level difference from Claude CLI for backend spawning.
- The RalphX Codex harness must make sandbox mode explicit in spawn metadata and raw logs.
- Provider/lane defaults and compatibility-locked workflows remain `danger-full-access` for compatibility with the current bridge.
- Standalone Chat runs against its private RalphX workspace with `workspace-write`; its MCP launch arguments still carry the backend-derived working directory, read roots, and `--filesystem-enforced 1` containment gate.
- Standalone PersonaBuilder remains on `danger-full-access` because its MCP workflow is not part of the Standalone Chat exception.
