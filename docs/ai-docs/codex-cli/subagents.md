# Codex Subagents

Official docs: `https://developers.openai.com/codex/subagents`

Snapshot notes:

- Codex has a first-class subagent feature instead of Claude `Task(...)`.
- Official docs present subagents as configurable Codex-native agents with their own model/tool/config behavior.
- Official model guidance explicitly calls out `gpt-5.4-mini` as suitable for lighter subagents.

RalphX notes:

- This is the vendor-native path for Codex ideation specialist / verifier delegation.
- RalphX should model delegation in provider-neutral terms and translate to Claude `Task(...)` vs Codex subagents per harness.
- RalphX exposes Codex subagents through its provider-neutral delegation and RX-native Team contracts.
