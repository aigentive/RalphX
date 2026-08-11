---
paths:
  - "frontend/tests/visual/**"
  - "frontend/tests/pages/**"
  - "frontend/tests/fixtures/**"
  - "frontend/tests/helpers/**"
---

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# Visual Testing

**Required Context:** code-quality-standards.md

## Rules

| # | Rule |
|---|------|
| 1 | **Page Object Model** — no raw selectors in spec files. Extend `BasePage` (`frontend/tests/pages/base.page.ts`). |
| 2 | **Mock parity** — component must render in web mode (`cd frontend && npm run dev:web`). Missing mock data → add to `frontend/src/api-mock/`. ❌ Log "blocked" and skip. |
| 3 | **Baseline required** — always `--update-snapshots` for new specs. |
| 4 | **Orphan detection** — grep for component imports + JSX usage before writing tests. ❌ Testing orphaned components. |
| 5 | **Playwright first** — prefer automated visual tests over manual/native visual QA; agents may start/stop only the scoped dev servers required by those tests. |
| 6 | **Computer Use requires an explicit request** — Native Tauri QA through Computer Use is prohibited unless the user explicitly requests it in the current request. Never infer permission from UI/theme scope, a test plan, or another rule. |

## Mock Parity (MUST FIX)

```
Component doesn't render in web mode?
├─ Missing mock data? → Add to frontend/src/api-mock/ → Include in commit
├─ Mock exists but broken? → Fix the mock → Include in commit
├─ Needs state manipulation? → Create test helper
└─ NEVER log "blocked" and skip — FIX IT
```

## File Patterns

| Type | Pattern | Location | Example |
|------|---------|----------|---------|
| Spec | `{feature}.spec.ts` | `frontend/tests/visual/{views\|modals\|states\|components\|polish\|theme-audit}/... (feature subdirs apply to views/modals/states/components; cross-cutting audits like polish/ and theme-audit/ place specs directly in the category dir)` | `kanban.spec.ts` |
| Spec subset | `{feature}-{subset}.spec.ts` | same | `kanban-cards.spec.ts` |
| Page object | `{feature}.page.ts` | `frontend/tests/pages/` | `kanban.page.ts` |
| Page sub-objects | `{feature}.page.ts` | `frontend/tests/pages/{views\|modals\|components}/` | `task-graph.page.ts` |
| Fixture | `{domain}.fixtures.ts` | `frontend/tests/fixtures/` | `tasks.fixtures.ts` |
| Helper | `{purpose}.helpers.ts` | `frontend/tests/helpers/` | `wait.helpers.ts` |

## File Size Limits

| Type | Max | Refactor At |
|------|-----|-------------|
| Spec | 200 | 150 |
| Page Object | 150 | 100 |
| Fixture | 100 | 80 |
| Helper | 50 | 40 |

## Playwright Commands

| Scenario | Command |
|----------|---------|
| New spec | `npx playwright test [spec] --update-snapshots` |
| Regression | `npx playwright test [spec]` |
| UI changed | `npx playwright test [spec] --update-snapshots` |
| Debug | `npx playwright test [spec] --debug` |

## Orphan Detection

| Check | Command | Orphan if |
|-------|---------|-----------|
| Imports | `grep -r "from.*ComponentName" frontend/src/ --include="*.tsx" \| grep -v index.ts` | 0 results |
| JSX | `grep -r "<ComponentName" frontend/src/ --include="*.tsx"` | 0 results |

## Workflow (Per Component)

```
1. Orphan check → grep for component usage
   → NOT used anywhere? → Skip
   → Used? → Continue

2. Mock parity → component renders in web mode?
   → No? → Fix mock (see decision tree above)
   → Yes? → Continue

3. Page object → frontend/tests/pages/{feature}.page.ts (extend BasePage)
4. Spec → frontend/tests/visual/{category}/{feature}/{feature}.spec.ts
5. Baseline → npx playwright test [spec] --update-snapshots
6. Verify → npx playwright test [spec] passes
7. Commit → test(visual): add [component] visual regression tests
```
