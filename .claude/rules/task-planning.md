---
paths:
  - "docs/handoffs/**"
  - ".artifacts/specs/**"
---

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# Task Planning Rules

**Required Context:** code-quality-standards.md | git-workflow.md

> Rules for designing tasks that can be executed atomically. Rules for designing tasks that can be executed atomically. Applies to ideation task-proposal design and PRD/plan decomposition.

## Compilation Unit Rule (CRITICAL)

**A task must be a complete compilation unit.** The code must compile and pass linters after each task.

### Detecting Compilation Units

Changes that **MUST** be in the same task:

| Change Type | Must Include |
|-------------|--------------|
| **Rename struct field** | All files that reference the old field name |
| **Rename function/method** | All call sites |
| **Change function signature** | All callers |
| **Add required field to struct** | All struct instantiations |
| **Remove export** | Remove all imports of that export |
| **Change type definition** | All usages of that type |

### Example: The Chicken-Egg Problem

**Bad PRD design (Phase 46 lesson):**
```
Task 1: Rename `comments` to `feedback` in CompleteReviewRequest struct
Task 2: Update handler to use `req.feedback` instead of `req.comments`
        blockedBy: [1]
```

**Why it fails:**
- Task 1 renames the field → handler now references non-existent `req.comments`
- Code won't compile after Task 1
- Task 2 can't "wait" for Task 1 because Task 1 is broken

**Correct design:**
```
Task 1: Rename `comments` to `feedback` in CompleteReviewRequest and update handler
        (Single compilation unit)
```

### Detection Checklist

Before finalizing a task, ask:

1. **Does this task rename or remove anything?**
   - Yes → Include all references in the same task

2. **Does this task change a type signature?**
   - Yes → Include all callers/usages in the same task

3. **Can I run the repo-appropriate validation/typecheck commands after JUST this task?**
   - No → Expand task scope until the task leaves the repo in a valid intermediate state

4. **Does the next task's `blockedBy` create a broken state?**
   - If Task N+1 is blocked by Task N, but Task N alone breaks the build, merge them

## Dependency Direction

### Blocking vs BlockedBy

| Field | Meaning | Example |
|-------|---------|---------|
| `blocking: [2, 3]` | Tasks 2 and 3 cannot start until this completes | Foundation work |
| `blockedBy: [1]` | This task cannot start until Task 1 completes | Dependent work |

**Rule:** If `blockedBy` would create an uncompilable intermediate state, the tasks must be merged.

### Valid Dependency Patterns

**Good: Layer boundaries**
```
Task 1: Add backend endpoint (compiles independently)
Task 2: Add frontend API wrapper (blockedBy: [1])
Task 3: Add UI component using wrapper (blockedBy: [2])
```
Each task crosses a layer boundary where the previous layer is stable.

**Good: Additive changes**
```
Task 1: Add new struct (nothing uses it yet - compiles)
Task 2: Add function using struct (blockedBy: [1])
Task 3: Wire function to handler (blockedBy: [2])
```
Each task adds without modifying existing code.

**Bad: Breaking changes split across tasks**
```
Task 1: Rename field X to Y (breaks all usages)
Task 2: Update usages of Y (blockedBy: [1]) ← WRONG
```
Task 1 is broken until Task 2 runs. Merge them.

## Cross-Layer Considerations

### Backend → Frontend Dependencies

When backend changes require frontend changes:

| Backend Change | Frontend Impact | Task Design |
|----------------|-----------------|-------------|
| **New endpoint** | New API wrapper needed | Separate tasks OK (additive) |
| **New response field** | Schema update needed | Separate tasks OK (additive) |
| **Rename response field** | Schema + all usages | **Same task** if strict types |
| **Remove response field** | Remove from schema + usages | **Same task** |

### Strict TypeScript Consideration

With strict TypeScript (`exactOptionalPropertyTypes`, etc.), adding a required field to a response type may break frontend compilation even if the backend is fine. Consider:

1. Make new fields optional initially (`field?: Type`)
2. Or include frontend schema update in same task as backend change

## Atomic Commit Boundaries

### What Can Be One Commit

- All files in a single compilation unit
- Test files for the code being added
- Type definitions AND their usages (if tightly coupled)

### What Should Be Separate Commits

- Backend changes + unrelated frontend changes
- New feature + refactoring of existing code
- Implementation + documentation updates

## Task Sizing Guidelines

| Task Size | Indicators | Action |
|-----------|------------|--------|
| **Too small** | Can't compile alone, needs next task | Merge with dependent task |
| **Right size** | Compiles, testable, 1-4 hours work | Keep as is |
| **Too large** | Multiple independent compilation units | Split at layer boundaries |

## Deriving Commit Messages

| Task Description Contains | Type |
|---------------------------|------|
| "create", "add", "implement", "new" | feat |
| "fix", "repair", "correct", "resolve" | fix |
| "rename", "update", "modify", "change" | feat or fix (based on context) |
| "refactor", "extract", "split", "reorganize" | refactor |
| "document", "readme", "template" | docs |
| "test", "spec", "verify" | test |

| Files Modified | Scope |
|----------------|-------|
| `src-tauri/**` | backend service/module name |
| `frontend/src/**` | frontend component/feature name |
| `plugins/app/ralphx-mcp-server/**` | mcp |
| `plugins/app/**` | plugin |
| `specs/**`, `docs/**` | docs |

## Validation Before Finalizing PRD

Run this mental check for each task:

```
For each task N:
  1. List all files it modifies
  2. For each modification:
     - Is it a rename/remove/signature change?
     - If yes: Are ALL affected files in this task?
  3. Simulate: If I run only this task's changes, does it compile?
  4. If no: Find the minimum additional changes needed
  5. Either expand this task OR merge with the blocking task
```

## Reference

- Git workflow: git-workflow.md
- Code quality: code-quality-standards.md
