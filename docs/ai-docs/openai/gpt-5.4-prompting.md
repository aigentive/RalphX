# GPT-5.4 Prompting Notes

Repo-local guidance for RalphX prompts targeting `gpt-5.4`, `gpt-5.4-mini`, or `gpt-5.4-nano`. Official OpenAI docs remain authoritative.

## Official Sources

- [Using GPT-5.4 — Prompting best practices](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.4#prompting-best-practices)
- [GPT-5.4 model guide](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.4)
- [GPT-5.4 system card](https://openai.com/index/gpt-5-4-thinking-system-card/) — safety/model behavior, not prompt-format authority
- [Unrolling the Codex agent loop](https://openai.com/index/unrolling-the-codex-agent-loop/) — Codex instruction layering

## Model-Specific Guidance

- Start with the smallest prompt that passes evals; add a block only for a measured failure mode.
- GPT-5.4 benefits from explicit early-session tool routing, prerequisite/dependency checks, completion criteria, and verification before high-impact actions.
- Use a clear output contract and the API `text.verbosity` control; do not let brevity omit required evidence or completion checks.
- Independent retrieval may run in parallel; dependent or irreversible steps remain sequential, followed by synthesis.
- For long-running/tool-heavy Responses flows, preserve assistant-item `phase`: `commentary` for intermediate updates and `final_answer` for completion. Do not add `phase` to user messages.
- If replaying history manually, round-trip original `phase` values; `previous_response_id` is usually simpler.
- Choose reasoning effort by task shape and eval evidence. Prompt completeness/tool/verification gaps should be fixed before globally raising effort.

## Variant Deltas

| Variant | Prompt implication |
|---|---|
| `gpt-5.4` | Suitable for broad, code-heavy, long-context, and multi-step agent workflows; explicit completion and evidence contracts improve reliability. |
| `gpt-5.4-mini` | More literal: put critical rules first, specify execution/ambiguity/output behavior, and avoid relying on implied steps. |
| `gpt-5.4-nano` | Use for narrow bounded tasks with closed outputs; route ambiguous planning/orchestration to a stronger model. |

## RalphX Application

- Keep stable agent roles/output contracts in canonical prompt files.
- Keep runtime authority and side-effect boundaries in the appropriate developer/system layer.
- Pass concrete task/session/project facts separately.
- For Codex integrations, preserve commentary/final phase semantics through streaming, persistence, replay, and compaction.
- Remove dead tool/path language instead of narrating migrations inside live prompts.

## Migration Discipline

1. Switch to the intended GPT-5.4 variant and preserve the current reasoning effort.
2. Run representative evals/traces.
3. Add only the smallest missing completion, dependency, tool-routing, evidence, or validation rule.
4. Re-run the same cases after each change.
