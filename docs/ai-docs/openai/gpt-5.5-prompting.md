# GPT-5.5 Prompting Notes

Repo-local guidance for RalphX prompts targeting the GPT-5.5 family. Do not treat GPT-5.5 as a prompt-identical replacement for GPT-5.4.

## Official Sources

- [Using GPT-5.5 — Prompting best practices](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.5#prompting-best-practices)
- [Using GPT-5.5 — Migration quickstart](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.5#migration-quickstart)
- [GPT-5.5 system card](https://openai.com/index/gpt-5-5-system-card/) — safety/evaluation context, not prompt-format authority

OpenAI publishes GPT-5.5 prompting guidance within the model guide; no separate standalone GPT-5.5 prompt-guidance page was identified in the official developer-doc index as of 2026-07-16.

## Model-Specific Guidance

- Prefer shorter, outcome-first prompts: define what good looks like, constraints, available evidence, required output, and stopping conditions; let the model choose an efficient path.
- Do not carry every step from an older prompt stack. Remove process instructions that narrow useful search or create mechanical behavior unless the path itself is a product requirement.
- Use absolute rules only for real invariants. Use decision criteria for when to search, ask, call tools, retry, or stop.
- Define personality and collaboration style separately and briefly; neither replaces goals, success criteria, tool rules, or stop rules.
- For long/tool-heavy streaming tasks, request a short preamble and preserve assistant `phase` values through replay.
- Make grounding and retrieval budgets explicit: say what needs evidence, what counts as enough, when another lookup is justified, and when to stop.
- Ask the model to validate work with the most relevant targeted test/typecheck/lint/build or rendered-artifact inspection.
- Use Structured Outputs for schemas where possible instead of duplicating complete schema definitions in prompt prose.

## Migration From GPT-5.4

1. Change only the model target and preserve the reasoning-effort baseline.
2. Run representative accuracy, token, latency, tool-loop, and user-visible output evals.
3. Remove redundant step-by-step scaffolding and repeated rules in small groups.
4. Add target-model instructions only for measured regressions.
5. Re-run the same cases after each prompt, effort, or verbosity change.

GPT-5.5 `text.verbosity: low` is proportionally more concise than the same setting on GPT-5.4; confirm required evidence and caveats are not lost.

## Suggested Contract Shape

Keep only sections that change behavior: role/personality, goal, success criteria, constraints/authority, tools/evidence, output, and stop rules.
