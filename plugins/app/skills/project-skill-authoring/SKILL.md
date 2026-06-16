---
name: project-skill-authoring
description: >
  Author RalphX learned project skills from bounded evidence. Use when drafting,
  distilling, importing, promoting, or reviewing project skills so candidates
  become reusable procedures with provenance, predicted effect, and human
  approval instead of one-session or one-commit summaries.
trigger: project skill draft | learned skill | skill distill | skill candidate | promote memory | import skill | GitHub PR skill
user-invocable: false
priority: 20
---

# Project Skill Authoring

Project skills are reusable project-scoped procedures. They are not memory facts,
commit summaries, PR summaries, changelog entries, or proof that a pattern is
already correct.

## Required Shape

Every draft must have:

- A class-level title that describes the reusable procedure.
- A bucket and stage from the RalphX enum set: `planning`, `verification`, `review`, `execution`, `merge`.
- Compact guidance that says when an agent should consider the skill.
- A body that explains what to do, what to verify, and what evidence triggered the draft.
- A falsifiable predicted effect.
- Provenance and a bounded source snapshot.

## Drafting Rules

1. Start from bounded evidence: task outcome, conversation outcome, selected PR metadata, selected memory, or an imported `SKILL.md`.
2. Do not read full diffs or large transcripts unless a caller explicitly requests deeper review and the path is approved.
3. Convert evidence into a reusable procedure. Do not create one skill per commit, PR, error string, or session.
4. Prefer umbrella skills with labeled subsections over narrow micro-skills.
5. Keep factual observations in memory. Promote only the reusable procedure into the project skill.
6. Keep generated drafts staged. Approval is the human gate before injection or export.

## Body Template

Use this body shape for generated drafts:

```markdown
## Authoring required

This is a staged draft from bounded RalphX evidence. Edit it before approval so
it describes a reusable project procedure, not just a past event.

## When to use

Use when ...

## Procedure

1. ...
2. ...
3. ...

## Verification

- ...

## Provenance

- Source: ...
- Evidence was bounded; full diffs/transcripts were not read unless stated.
```

## Quality Bar

Reject or rewrite a candidate when:

- It only says to "review the prior commit/PR".
- It depends on a single title without concrete reusable steps.
- The predicted effect cannot be checked later.
- The body copies raw memory instead of transforming it into procedure.
- The scope is too broad for the evidence.
