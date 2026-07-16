# OpenAI Integration Notes

This directory is the local OpenAI model and prompting surface for RalphX.

Scope:

- Official OpenAI docs remain the source of truth when current vendor behavior matters.
- Local files here capture RalphX-specific conclusions so prompt work does not have to be rediscovered every session.
- Use the guide matching the configured target model; Codex harness integration and model-family prompting are separate concerns.

Current files:

- `gpt-5.4-prompting.md` — GPT-5.4 behavior, variants, phase handling, and explicit completion/tool-routing guidance.
- `gpt-5.5-prompting.md` — GPT-5.5 outcome-first prompting, validation, retrieval budgets, and migration deltas from 5.4.
- `gpt-5.6-prompting.md` — GPT-5.6 family lean-prompt, autonomy, tool-routing, reasoning, state, and migration guidance.

Primary official sources:

- GPT-5.4 model/prompt guide: `https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.4`
- GPT-5.4 system card: `https://openai.com/index/gpt-5-4-thinking-system-card/`
- GPT-5.5 model/prompt guide: `https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.5`
- GPT-5.5 system card: `https://openai.com/index/gpt-5-5-system-card/`
- GPT-5.6 family prompt guide: `https://developers.openai.com/api/docs/guides/prompt-guidance-gpt-5p6`
- GPT-5.6 system card: `https://deploymentsafety.openai.com/gpt-5-6`
- Codex instruction layering: `https://openai.com/index/unrolling-the-codex-agent-loop/`
