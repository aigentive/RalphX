<release_proposal_task>
Recommend the next RalphX.app release version from the provided deterministic release evidence packet.

Use only the provided evidence and versioning policy.
</release_proposal_task>

<style_goals>
- factual, crisp, and decision-oriented
- optimized for a human release owner choosing accept or override
- compact like an engineering recommendation memo, not marketing copy
- no hype, no invented claims, no filler
- technical and explicit about tradeoffs when the evidence is mixed
</style_goals>

<output_format>
# RalphX.app Release Proposal

One short summary sentence.

## Recommendation
- Current released version: <version>
- Recommended bump: <patch|minor|major>
- Proposed version: <one of the provided candidate versions>
- Confidence: <high|medium|low>

## Why This Bump
- 2-5 bullets tying the recommendation to concrete shipped changes, with clickable commit links when the evidence is known

## Human Review
- 1-3 bullets for what a human should sanity-check before accepting, or when they should override up/down
</output_format>

<source_of_truth>
- Treat the deterministic release evidence packet as the primary source of truth.
- Treat Raw commit bodies as the primary narrative source when they contain meaningful merge bullets.
- Use Commit subjects and Diff stat only as secondary context when the raw bodies are sparse.
- Do not infer user-visible behavior from file names alone.
- Choose exactly one of the candidate versions provided in the packet.
</source_of_truth>

<writing_rules>
- Output Markdown only.
- Recommend the smallest justified bump under the supplied versioning policy.
- Do not invent a version number outside the provided candidates.
- Treat `0.x.y` as an intentionally long-lived release line with unbounded numeric minor and patch segments; do not treat large numbers such as `0.100.0` as a reason to jump to `1.0.0`.
- Do not recommend `major` unless the evidence clearly proves the manually approved 1.0-level milestone or compatibility-reset bar stated in the packet.
- A `major` recommendation is a stop signal for automation, not a normal daily-release outcome.
- Do not inflate the recommendation just because the repo has high internal change velocity.
- Use `minor` when the evidence supports a meaningful new or expanded shipped surface.
- Use `patch` when the evidence is primarily fixes, polish, internal maintenance, dependency churn, or limited incremental expansion.
- Keep the recommendation grounded in shipped behavior and release surface, not in repository churn.
- Use inline traceability on Why This Bump bullets with exact Markdown commit links from the `Commit references` section, for example `[42b26250](https://github.com/owner/repo/commit/<full-sha>)`.
- Do not put commit links or short SHAs in backticks; the traceability reference should be a clickable Markdown link.
- Make the Human Review section practical: mention ambiguity, scope boundaries, or reasons a human might override the recommendation.
- Do not write like a changelog or full release notes; this is a version recommendation memo.
- Avoid generic wording like `several improvements`, `various updates`, or `mixed changes`.
- The opening summary sentence should name the recommended bump and 1-2 concrete reasons.
- Keep the `- Proposed version: <x.y.z>` line exact so downstream tooling can parse it.
</writing_rules>
