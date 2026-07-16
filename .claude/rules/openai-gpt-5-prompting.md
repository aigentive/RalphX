---
paths:
  - "scripts/prompts/**"
  - "scripts/generate-release-notes.sh"
  - "agents/**"
  - "docs/ai-docs/openai/**"
  - "src-tauri/src/infrastructure/agents/**"
  - "src-tauri/src/application/chat_service/**"
---

> **Maintainer note:** Keep this file compact. Prefer one-line rules, links to source docs, and explicit non-negotiables over prose.

# OpenAI GPT-5 Prompt Routing

Select guidance by the actual configured target model; the Codex harness and model family are separate axes.

| Target model | Required local guide | Primary official source |
|---|---|---|
| `gpt-5.4*` | `docs/ai-docs/openai/gpt-5.4-prompting.md` | [Using GPT-5.4](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.4) |
| `gpt-5.5*` | `docs/ai-docs/openai/gpt-5.5-prompting.md` | [Using GPT-5.5](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.5) |
| `gpt-5.6*` | `docs/ai-docs/openai/gpt-5.6-prompting.md` | [Prompting guidance for GPT-5.6 Sol/family](https://developers.openai.com/api/docs/guides/prompt-guidance-gpt-5p6) |

## Shared Rules

- Preserve instruction layers: stable role/contract, run-level developer guardrails, and task facts remain separate.
- Define the user-visible outcome, success criteria/completion bar, hard constraints, tool/authority boundaries, required evidence, output shape, and stop rules.
- State each instruction once; remove contradictions, stale process scaffolding, irrelevant examples, and irrelevant tools before adding more prose.
- Use absolute language only for true invariants; use decision criteria for judgment calls.
- Require relevant validation and define behavior for missing, partial, or conflicting evidence.
- For migrations, switch the model while preserving the current reasoning baseline, run representative evals, then make one measured prompt/effort change at a time.
- Do not apply the GPT-5.4 guide wholesale to GPT-5.5 or GPT-5.6; use only shared principles plus the selected model's documented deltas.
- System/model cards support safety and behavior claims; they are not substitutes for prompt-authoring guidance.
