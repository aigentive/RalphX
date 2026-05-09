<release_notes_task>
Draft polished Markdown release notes for RalphX.app, a native macOS desktop app for AI-driven software development.

Use only the provided deterministic release evidence packet and supporting facts.
</release_notes_task>

<style_goals>
- factual, crisp, and professional
- grouped by user-visible impact first, with developer/maintainer work clearly separated near the bottom
- compact like an engineering changelog, but easier to scan than a wall of text
- no hype, no invented claims, no filler
- suitable for multiple audiences at once: public readers, active users, contributors, and maintainers
- public-facing for a developer community: assume the reader is technical, values precision, and does not need non-technical simplification
- preserve technical specificity and explicit commit traceability
- keep each bullet scoped to one coherent product area, not a grab-bag of unrelated fixes
</style_goals>

<output_format>
# RalphX.app v<version>

One short summary sentence.

## User-Facing Changes
- 2-5 bullets for the most important changes someone sees when downloading, installing, opening, or using RalphX.app

## Fixes And Polish
- optional bullets for smaller user-visible UX/runtime fixes

## Developer And Maintainer Changes
- optional bullets for internal, CI, release automation, docs, config, scaffolding, or contributor-facing work
- include this section after all user-facing sections when developer-facing work is worth mentioning

## Other
- optional
- use only for real items worth mentioning that do not fit naturally under the sections above

## Known Issues
- optional
- only include issues if the provided facts justify them
</output_format>

<source_of_truth>
- Treat the deterministic release evidence packet as the primary source of truth.
- Treat Raw commit bodies as the primary narrative source when they contain meaningful merge bullets.
- Use Commit subjects and Diff stat only as secondary context when the raw bodies are sparse.
- Do not infer user-visible behavior from file names alone.
- Do not assume every `feat:` bullet represents a net-new surface; many are expansions of existing capabilities.
</source_of_truth>

<writing_rules>
- Output Markdown only.
- Do not mention items that are not supported by the provided facts.
- Do not claim a change is user-visible unless the facts support that.
- Prefer grouping by capability area rather than by file.
- Never include raw commit subjects such as `feat: ...` or `fix: ...` in the final release notes.
- Include inline traceability on bullets using exact Markdown commit links from the `Commit references` section, for example `[42b26250](https://github.com/owner/repo/commit/<full-sha>)`.
- Do not put commit links or short SHAs in backticks; the traceability reference should be a clickable Markdown link.
- For broad squash or merge ranges, repeating the same commit link across multiple bullets is acceptable.
- Prefer `improves`, `expands`, `reworks`, `upgrades`, `fixes`, `stabilizes`, or `defaults` unless the evidence clearly supports `adds` or `introduces`.
- When the evidence packet provides a strong concrete example, fold that example into the bullet instead of staying abstract.
- Keep bullets denser than marketing copy: one capability claim plus one concrete example is ideal.
- Do not combine unrelated fixes into a single catch-all bullet just to reduce bullet count.
- If smaller fixes do not naturally cluster, prefer shorter separate bullets or omit the weakest items instead of inventing a generic umbrella.
- Every User-Facing Changes bullet and every Fixes And Polish bullet should include at least one concrete visible example when the evidence packet provides one.
- For broad first-release or squash-merge ranges, prefer 4-5 strong User-Facing Changes bullets when the evidence supports them.
- If the range looks like a first release or a broad release cut, frame that professionally and confidently in the summary without sounding defensive, apologetic, or self-undermining.
- Do not use phrases like `unusually broad`, `mixed bag`, `catch-all`, `messy`, `still evolving`, or similar wording that makes the release sound accidental or poorly shaped.
- Prefer opener patterns like `This first 0.1.0 release...`, `This initial 0.1.0 release...`, or `RalphX.app 0.1.0 focuses on...` when they fit the evidence.
- The opening summary sentence must name 2-3 concrete release themes and must not fall back to generic phrases like `consolidates the current baseline`, `brings various improvements`, or `tightens several workflows`.
- Keep User-Facing Changes and Fixes And Polish focused on runtime, UI, workflow, install, and release outcomes.
- Put user-facing changes before developer-facing changes.
- Keep developer-only work out of User-Facing Changes and Fixes And Polish; use Developer And Maintainer Changes instead.
- Do not mix user-facing and developer-facing changes in the same bullet.
- If a release contains both product-visible work and internal release/CI/docs/config work, the product-visible work must appear first and the internal work must appear under Developer And Maintainer Changes.
- Lead each bullet with the visible surface or workflow that changed, not the underlying implementation mechanism.
- Avoid opening bullets with internals such as `backend-managed`, `native snapshots`, `canonical agent.yaml`, or similar repository-facing terminology unless there is no clearer user-facing phrasing.
- Preserve technical detail when it helps comprehension, but do not drift into repo-maintainer jargon.
- The same notes should work for public readers, contributors, and maintainers, so keep them concrete and technical without becoming repo-internal.
- Summarize and group the raw commit-body bullets; do not simply restate the merge title and then guess from file names.
- Prefer direct, specific verbs when the evidence is strong:
  - use `shows`, `surfaces`, `renders`, `expands`, `reworks`, `defaults`, `refreshes`, `stabilizes`, `fixes`
  - avoid weak verbs like `tightens`, `clarifies`, `supports`, or `consolidates` unless the evidence genuinely does not justify stronger wording
- If the packet describes a concrete UI state change, name that state change instead of abstracting it away.
- Favor the strongest visible example in the bullet, not the most generic one.
- Use Other sparingly. Keep it short and high-signal, and omit deeply repository-specific maintenance details unless they clearly matter to users, operators, contributors, or maintainers.
</writing_rules>
