<stable_release_notes_task>
Combine several already-published per-build release notes for RalphX.app, a native macOS desktop app for AI-driven software development, into ONE cumulative Stable release note.

The Stable release being promoted bundles every build since the previous Stable release. Readers of this note are upgrading directly from the previous Stable version, so they never saw the intermediate per-build notes.

Use only the provided packet: the ordered per-build notes and the supporting commit subjects.
</stable_release_notes_task>

<style_goals>
- factual, crisp, and professional
- grouped by user-visible impact first, with developer/maintainer work clearly separated near the bottom
- compact like an engineering changelog, but easier to scan than a wall of text
- no hype, no invented claims, no filler
- suitable for multiple audiences at once: public readers, active users, contributors, and maintainers
- public-facing for a developer community: assume the reader is technical, values precision, and does not need non-technical simplification
- preserve technical specificity and explicit commit traceability
- reads as one coherent release, not as concatenated per-build notes
</style_goals>

<output_format>
One short summary sentence.

## User-Facing Changes
- 3-7 bullets for the most important changes someone sees when downloading, installing, opening, or using RalphX.app across the whole Stable span

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
- only include issues if the provided per-build notes justify them
</output_format>

<no_heading_rule>
- Do NOT emit a top-level `# RalphX.app v<version>` heading line.
- The release title is supplied by GitHub; start directly with the summary sentence.
</no_heading_rule>

<source_of_truth>
- Treat the per-build notes as the primary source of truth; they are already curated and reviewed.
- Use the commit subject list only as secondary evidence to fill a gap when a per-build note is missing or sparse.
- Do not introduce any change that is not present in the provided per-build notes or commit subjects.
- Do not infer user-visible behavior from file names alone.
</source_of_truth>

<combination_rules>
- Merge and dedupe: when several builds touched the same capability, emit ONE bullet describing the end state a Stable upgrader receives, not the incremental history.
- When a later build fixed or reworked something an earlier build introduced within the same span, describe only the final shipped behavior. Do not narrate the intermediate churn.
- Group by product area, not by version. Do NOT add per-version subheadings.
- The only exception: if a change genuinely only makes sense versioned (for example a migration or a behavior that changed mid-span in a way users must know about), state the version inline in that bullet.
- Preserve the strongest concrete example from the source bullets when collapsing several bullets into one.
- Drop the weakest items rather than inventing a generic umbrella bullet to absorb them.
- Rank the merged bullets by user impact across the whole span, not by the order the builds shipped.
- A Stable span can be large; prefer a tight, high-signal note over an exhaustive one.
</combination_rules>

<writing_rules>
- Output Markdown only.
- Do not mention items that are not supported by the provided per-build notes.
- Do not claim a change is user-visible unless the source notes support that.
- Keep the exact Markdown commit links from the source bullets intact when you carry a bullet forward; do not rewrite, shorten, or drop them.
- When merging several source bullets, carry the relevant commit links from each into the merged bullet.
- Do not put commit links or short SHAs in backticks; the traceability reference should stay a clickable Markdown link.
- Never include raw commit subjects such as `feat: ...` or `fix: ...` in the final note.
- Prefer `improves`, `expands`, `reworks`, `upgrades`, `fixes`, `stabilizes`, or `defaults` unless the evidence clearly supports `adds` or `introduces`.
- Keep bullets denser than marketing copy: one capability claim plus one concrete example is ideal.
- Do not combine unrelated fixes into a single catch-all bullet just to reduce bullet count.
- Keep User-Facing Changes and Fixes And Polish focused on runtime, UI, workflow, install, and release outcomes.
- Put user-facing changes before developer-facing changes; do not mix the two in the same bullet.
- Lead each bullet with the visible surface or workflow that changed, not the underlying implementation mechanism.
- Avoid opening bullets with repository-facing internals unless there is no clearer user-facing phrasing.
- The opening summary sentence must name 2-3 concrete themes of the Stable span and must not fall back to generic phrases like `consolidates the current baseline`, `brings various improvements`, or `tightens several workflows`.
- Do not use phrases like `unusually broad`, `mixed bag`, `catch-all`, `messy`, or `still evolving` that make the release sound accidental.
- Do not describe the note as cumulative, combined, or aggregated; just write the release note.
- Prefer direct, specific verbs when the evidence is strong:
  - use `shows`, `surfaces`, `renders`, `expands`, `reworks`, `defaults`, `refreshes`, `stabilizes`, `fixes`
  - avoid weak verbs like `tightens`, `clarifies`, `supports`, or `consolidates` unless the evidence genuinely does not justify stronger wording
- Use Other sparingly. Keep it short and high-signal.
</writing_rules>
