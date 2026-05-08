# Codex Models

Official docs: `https://developers.openai.com/codex/models`

Snapshot notes:

- Official docs recommend `gpt-5.5` as the starting point for complex reasoning and coding.
- Official docs list `gpt-5.4` and `gpt-5.4-mini` as current lower-cost / lower-latency options, with `gpt-5.4-mini` positioned for coding, computer use, and subagents.
- Official docs list `gpt-5.3-codex` as the current non-deprecated coding-specialized Codex model; older `gpt-5-codex`, `gpt-5.2-codex`, and GPT-5.1 Codex variants are marked deprecated.
- The docs state that Codex CLI and SDK use the same model configuration surface.

RalphX notes:

- RalphX should default primary Codex lanes to `gpt-5.5` with `xhigh` reasoning.
- Initial preferred verifier / specialist subagent default should be `gpt-5.4-mini` unless audit evidence says a stronger model is required.
- Keep selectable Codex presets aligned to the current RalphX-supported set: `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex`, and `gpt-5.3-codex-spark`.
- Codex reasoning effort should be configured separately from the model name.
