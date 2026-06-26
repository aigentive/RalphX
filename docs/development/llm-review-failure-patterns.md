# LLM Review Failure Patterns

## Purpose

This note captures why a capable model can miss branch bugs on a first fixing pass, especially in RalphX stateful workflows, and the review pattern we should use to reduce repeat misses.

The companion durable rule is `.claude/rules/stateful-workflow-review.md`.

## Common First-Pass Misses

| Pattern | What it looks like | Why it happens | Countermeasure |
|---|---|---|---|
| Patch-intent anchoring | The model validates the intended fix instead of hunting for adjacent regressions. | The prompt frames the task as "fix this" rather than "disprove success." | Add a false-success pass before handoff. |
| Local helper trust | A helper name such as `current_attempt` or `validation_cache` is accepted as authoritative without tracing its query semantics. | Models overweight names and underweight temporal state. | Require attempt-scoped proof from persistence reads to final effects. |
| Boolean collapse blindness | Errors, stale reads, zero-step states, and true success all collapse into one success path. | Branches look simpler than the runtime states they represent. | Make reads fail closed and test error paths. |
| Temporal myopia | Older attempts, cached HEAD results, or startup recovery paths are treated as if they belong to the current run. | LLMs reason well over static diffs but weaker over event history. | Review stale attempt, duplicate call, retry, re-entry, and recovery cases separately. |
| Evidence confusion | Prior validation evidence is reused for new code or a new attempt. | The model sees a green test name or cache field and infers live relevance. | Tie validation evidence to the current run, current revision, and current attempt. |
| Event-ordering blind spot | Completion events, webhooks, auto-commit, or UI updates fire before final authority checks. | Side effects are reviewed as logging rather than state transitions. | Enforce authority-before-effects ordering. |
| Test-as-confirmation bias | Tests prove the happy path still passes but do not falsify the suspected false-success case. | The model writes tests around the implemented branch rather than the failure model. | Add at least one regression that fails against the old behavior. |
| Prompt/runtime split-brain | Prompts ask agents to send fields or use tools that the backend schema does not accept. | Prompt edits are treated as prose, not API clients. | Run prompt-schema contract review for examples, tool names, and payload fields. |
| Path-taint underweighting | Worktree paths, repo-relative paths, and cache keys are trusted because they are internal-looking. | The model misses that env, DB, MCP, or settings state can influence filesystem sinks. | Validate sinks at the sink with process-owned roots and fixed entry lists. |
| Documentation drift acceptance | Handoffs and comments describe the intended invariant, while code implements a weaker one. | Natural language sounds authoritative and lowers scrutiny. | Treat docs as hypotheses and verify against production code/tests. |

## Multi-Pass Review Recipe

Run these passes explicitly for completion gates, retries, recovery, validation caches, state-machine transitions, and execution prompts.

1. Intent pass: summarize the intended invariant in one sentence.
2. State pass: list every state that can reach the changed code, including stale, retry, recovery, duplicate, zero-step, and failed-read states.
3. Authority pass: identify the final source of truth and prove all side effects happen after it.
4. Freshness pass: prove evidence is scoped to the current attempt, revision, worktree, and run.
5. Failure pass: force every repository/query/cache/tool error to a fail-closed outcome unless a narrower rule proves otherwise.
6. Prompt contract pass: compare agent prompt examples with live tool schemas and backend request types.
7. Test falsification pass: name which tests would fail against the old bug and which production path they exercise.

## Review Prompt

Use this prompt as a reusable second pass:

```text
Review this diff for false success. Assume the intended fix is incomplete.

Find any path where stale attempts, stale cache data, zero-step state, failed reads,
duplicate calls, recovery/re-entry, or prompt/tool schema drift can make the system
report success before the authoritative current run is actually complete.

For each issue, show:
- the production path,
- the state or ordering that triggers it,
- why current tests would miss it,
- the smallest regression test that would fail before the fix.
```

## Expected Outcome

The goal is not more prose in every prompt. The goal is a durable review habit: models should stop once per stateful change and actively try to disprove their own success story before handoff.
