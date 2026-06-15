# Project Skills

Project skills are reusable, project-scoped procedures that RalphX can learn from outcomes, import from a manifest, promote from memory, approve, inject into future agents, and optionally export to the target repository.

They are different from factual memory:

| Surface | Stores | Example |
|---|---|---|
| Memory | Facts and observations about this project or run | "This repo uses a custom merge validation script." |
| Project skill | Reusable procedure for future agents | "Before approving merge output, run the custom validation script and inspect its failure class." |

Use the **Skills** sidebar item to review staged skills, approve/reject them, inspect report cards, import skill manifests, promote memory, and export approved skills.

## Enabling The Skills UI

The Skills product surface is controlled by **Settings -> App Preferences -> Skills**.

When enabled:
- the main sidebar shows **Skills**
- Agent conversations can show a right-side **Skills** artifact tab
- the Skills tab can process an older conversation on demand

When disabled:
- the main sidebar **Skills** button is hidden
- conversation Skills shortcuts/tabs are hidden
- existing learned-skill data remains in the database

## Normal Workflow

1. Let agents run normally.
2. Open **Skills**.
3. Click **Distill** to stage eligible learned skills from recorded outcomes.
4. Review staged skills.
5. Click **Approve** for skills you want agents to use.
6. Optionally **Pin** important approved skills.
7. Use **Preview export** before writing `.claude/skills` files.

Approved skills are eligible for runtime injection and export. Staged skills are review candidates only.

## Conversation Skills

In an Agent conversation, open the right-side artifact pane and choose **Skills**.

The conversation Skills tab shows skills connected to that conversation by:
- provenance from a generated/staged skill
- usage events from injected approved skills

Clicking refresh/process on that tab processes the current conversation on demand. This is intended for older chats that ran before automatic skill processing existed.

## Export To `.claude/skills`

Export is an explicit opt-in sink. It writes approved or pinned project skills into the active target repo at:

```text
.claude/skills/<skill-slug-hash>/SKILL.md
```

Export is designed as a reviewable git change, not a silent runtime dependency.

### Requirements

Before clicking **Export**:

1. At least one skill is approved or pinned.
2. **Export enabled** is switched on in the Skills panel.
3. The target project is a git repository.
4. The target project is on a named review branch.
5. The branch is not `main`, `master`, or `trunk`.
6. The target worktree is clean.

Preview export is read-only. Export apply writes files only after these checks pass.

### Recommended Export Flow

```bash
git switch -c ralphx/export-skills
git status --short
```

Then in RalphX:

1. Open **Skills**.
2. Approve or pin the skills you want.
3. Turn on **Export enabled**.
4. Click **Preview export**.
5. Review the target paths and "will write" rows.
6. Click **Export**.

After export:

```bash
git status --short
git diff -- .claude/skills
git add .claude/skills
git commit -m "docs: export RalphX project skills"
```

### Export Troubleshooting

| Symptom | Meaning | Fix |
|---|---|---|
| Export button is disabled | Export is not enabled or no approved/pinned skill exists | Approve/pin a skill and turn on **Export enabled** |
| "requires a git repository review branch" | Target project is not a git checkout | Register a git-backed project |
| "requires a named review branch" | Detached HEAD | Create/switch to a normal branch |
| "refuses to write directly on protected branch" | Branch is `main`, `master`, or `trunk` | Create a review branch first |
| "requires a clean review branch" | Uncommitted or untracked files exist | Commit, stash, or delete the files, then retry |
| Preview shows 0 files | No approved or pinned skills are export-eligible | Approve or pin skills first |

If export wrote files once, your worktree is now dirty by design. Commit or remove those `.claude/skills` files before running export again.

## Promote Memory

**Promote memory** turns one existing memory row into a staged project skill candidate.

It does not copy memory directly into runtime guidance. You must rewrite the useful fact into a reusable procedure. The original memory remains unchanged and becomes provenance on the staged skill.

### Required Inputs

| Field | Required | Meaning |
|---|---:|---|
| Memory ID | Yes | ID of an existing active memory row in the same project |
| Skill title | No | Human-readable skill name; defaults to the memory title |
| Bucket | Yes | Broad use area, commonly `review`, `execution`, `merge`, or `verification` |
| Stage | Yes | Sub-area within the bucket, commonly `review` |
| Compact guidance | Yes | Short guidance used for matching/selection |
| Skill body | Yes | The reusable procedure agents should follow |
| Predicted effect | Yes | Expected improvement from applying this skill |

### Good Promotion Example

Memory:

```text
Merge validation failed twice because the project requires scripts/validate_sqlite_migrations.py after schema changes.
```

Promoted skill:

```text
Title: Validate SQLite migrations before merge
Bucket: merge
Stage: review
Compact guidance: When a change touches SQLite migrations, validate migration ordering before approving merge output.
Body:
- Check for new migration files under src-tauri/src/infrastructure/sqlite/migrations.
- Run python3 scripts/validate_sqlite_migrations.py before merge approval.
- If validation fails, classify it as a migration-ordering issue before requesting code changes.
Predicted effect: Reduces repeated merge validation failures on schema-changing tasks.
```

### Bad Promotion Example

Do not promote raw facts as the skill body:

```text
The repo had a migration failure on June 14.
```

That belongs in memory. A skill must tell future agents what to do differently.

### Promotion Troubleshooting

| Symptom | Meaning | Fix |
|---|---|---|
| Promote button is disabled | Required fields are empty | Fill Memory ID, compact guidance, body, and predicted effect |
| "memory entry not found" | The ID does not exist | Use a real memory ID from the current project |
| Cross-project rejection | The memory belongs to another project | Promote only same-project memories |
| Staged skill appears but agents do not use it | Staged skills are not injected | Approve the skill first |

## Import Manifest

Import accepts a JSON manifest and stages eligible rows after a backend preview.

Minimum shape:

```json
{
  "candidates": [
    {
      "externalId": "optional-source-id",
      "title": "Review migration validation",
      "bucket": "review",
      "stage": "review",
      "scopePaths": ["src-tauri/**"],
      "compactGuidance": "Check migration validation before approving schema changes.",
      "bodyMarkdown": "Procedure text goes here.",
      "predictedEffect": "Reduces repeated schema validation failures.",
      "provenance": {
        "source": "manual_import"
      },
      "sourceSnapshot": {
        "capturedFrom": "handoff"
      }
    }
  ]
}
```

Preview is fail-closed: invalid rows stay invalid until fixed. Apply stages only eligible rows and preserves the source snapshot in provenance.

## What To Expect

| Action | Result |
|---|---|
| Distill | Creates staged candidates from eligible outcomes |
| Approve | Makes a staged skill available for future use |
| Reject | Keeps provenance but prevents use |
| Pin | Keeps an approved skill prominent and export-eligible |
| Archive | Hides a skill from active lists |
| Export | Writes approved/pinned skills to `.claude/skills` on a clean review branch |
| Promote memory | Creates a staged skill candidate from rewritten procedural guidance |

Project skills are still conservative: reporting is descriptive, underpowered skills should not be treated as proven winners, and export remains opt-in.
