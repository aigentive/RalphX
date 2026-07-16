# GPT-5.6 Family Prompting Notes

Repo-local guidance for prompts targeting `gpt-5.6`, `gpt-5.6-sol`, `gpt-5.6-terra`, or `gpt-5.6-luna`. OpenAI's dedicated guide explicitly covers GPT-5.6 Sol and the GPT-5.6 family.

## Official Sources

- [Prompting guidance for GPT-5.6 Sol/family](https://developers.openai.com/api/docs/guides/prompt-guidance-gpt-5p6)
- [Using GPT-5.6](https://developers.openai.com/api/docs/guides/latest-model)
- [GPT-5.6 system card](https://deploymentsafety.openai.com/gpt-5-6) — safety, prompt-injection, confirmation-policy, and destructive-action evidence; not a prompt-format guide

## Model-Specific Guidance

- Favor lean prompts. State each instruction once, expose only relevant tools, and remove obsolete scaffolding/examples one group at a time while rerunning evals.
- Define outcomes, important constraints, available evidence, completion bar, authority/approval boundaries, output shape, and stop rules; leave efficient path selection to the model.
- GPT-5.6 is more concise by default than GPT-5.5. Re-evaluate broad “be concise” instructions and use `text.verbosity` for the default detail level.
- Define autonomy once: safe in-scope local work may continue; external, destructive, costly, or scope-expanding actions require confirmation.
- Tool routing should name prerequisites, bounded stages, documented return fields, fallbacks, retry limits, stop conditions, and handoffs only where they change behavior.
- Use Programmatic Tool Calling for bounded deterministic reduction/aggregation, not merely because calls are parallel or dependent; keep semantic judgment, approvals, citations, and final validation direct.
- Preserve sparse user-visible progress updates, assistant phase values, compaction semantics, and reasoning context across long-running workflows.
- Persist reasoning only while objectives/assumptions/priorities remain relevant; stale reasoning can anchor later turns.
- Define evidence/citation boundaries and retrieval budgets; distinguish sourced facts from inference or creative wording.

## Reasoning And Variants

- Preserve the current GPT-5.4/5.5 effort as the migration baseline, then compare the same setting and one level lower.
- Use `low` for latency-sensitive work when quality holds, `medium` as a balanced start, and higher efforts only when evals show a gain; reserve `max` for the hardest quality-first tasks.
- Select `sol`, `terra`, or `luna` by workload capability/cost/volume needs; the prompt contract should not assume one variant's deployment economics.

## Migration Workflow

1. Switch the model and preserve current reasoning effort.
2. Run representative evals before changing the prompt.
3. Remove obsolete scaffolding, repeated instructions, and irrelevant tools.
4. Add only the smallest targeted rule that fixes a measured regression.
5. Re-run evals after each prompt or reasoning change.

Do not rewrite a working prompt stack all at once; otherwise model, effort, prompt, tool-set, and runtime effects cannot be separated.
