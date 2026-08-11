---
paths:
  - "frontend/src/**/*.{ts,tsx,js,jsx}"
  - "src-tauri/src/**/*.rs"
  - "plugins/app/**/*.{ts,js}"
---

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# Task Management Enforcement

## When to Use Task Tools

Use TaskCreate/TaskUpdate/TaskList when ANY criteria met:

- **>3 files** modified
- **Any refactoring** (rename, extract, restructure)
- **Any extraction** (new hook, component, service, helper)
- **>100 LOC** changed
- **Multi-step** implementation
- **Architectural changes** (new patterns, DI changes)

## Workflow

```
TaskCreate → TaskUpdate(in_progress) → Work → TaskUpdate(completed)
```

1. **Create tasks upfront** — Before starting work
2. **Mark in_progress** — When you begin work on a task
3. **Mark completed** — When task is done

## Benefits

- Enables progress tracking
- Prevents scope creep
- Makes complex work visible
- Allows resumption if interrupted
