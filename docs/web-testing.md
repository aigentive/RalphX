# Web Target and Browser Testing Guide

This document covers how to run RalphX in web mode (without Tauri) for browser automation testing with Playwright.

## Overview

RalphX can run in two modes:

| Mode | Command | Backend | Events | Use Case |
|------|---------|---------|--------|----------|
| **Tauri Mode** | `cd frontend && npm run tauri dev` | Real Rust backend | Tauri IPC events | Normal development |
| **Web Mode** | `cd frontend && npm run dev:web` | Mock API (read-only) | In-memory events | Visual testing, Playwright |

Web mode enables browser automation testing by mocking the Tauri backend, allowing the React frontend to render with mock data in any browser.

---

## Running Web Mode

### Start the Dev Server

```bash
cd frontend && npm run dev:web
```

This starts Vite with `--mode web`, which:
- Uses mock API implementations from `frontend/src/api-mock/`
- Aliases Tauri plugins to mock implementations in `frontend/src/mocks/`
- Runs on port 5173 (separate from Tauri's port 1420)

### Build for Web

```bash
cd frontend && npm run build:web
```

Outputs to `dist-web/` directory for deployment or static testing.

### Access in Browser

Navigate to `http://localhost:5173` in any browser. The app will:
1. Detect it's not in Tauri (no `window.__TAURI_INTERNALS__`)
2. Use mock API for all backend calls
3. Use in-memory event bus for events
4. Load mock data (projects, tasks, proposals, etc.)

---

## What Works in Web Mode

### Fully Functional

| Feature | Notes |
|---------|-------|
| **UI Rendering** | All components render with mock data |
| **Navigation** | All routes and views accessible |
| **Kanban Board** | Task cards, columns, drag-drop (visual only) |
| **Ideation View** | Sessions, proposals, chat display |
| **Activity View** | Activity events with mock data |
| **Form Submission** | Forms submit without error (no-op) |
| **Theme Switching** | Light/dark mode works |
| **Animations** | All Framer Motion animations work |

### Mock Data Available

The mock API provides factory-generated data for:
- Projects (with default selection)
- Tasks (multiple statuses, mock content)
- Ideation sessions and proposals
- Plan artifacts
- Activity events
- Execution status
- Reviews

### Read-Only Behavior

| Action | Behavior |
|--------|----------|
| Create task | Returns mock success, no persistence |
| Update task | Returns mock success, state may update locally |
| Delete task | Returns mock success, no actual deletion |
| Move task | Visual only, no backend update |
| Send chat message | UI shows sending, mock response |

---

## Limitations

### Not Available in Web Mode

| Feature | Reason |
|---------|--------|
| **Real data persistence** | No SQLite database |
| **Agent execution** | No native agent harness backend |
| **File system access** | Mocked plugin returns empty |
| **Native dialogs** | Mocked to return defaults |
| **Auto-updates** | Mocked updater plugin |
| **Global shortcuts** | Mocked to no-op |

### Known Differences

1. **Data is reset on refresh** - Mock data regenerates each load
2. **No real-time updates** - Mock event bus doesn't receive backend events
3. **Forms appear to work but don't persist** - Good for visual testing, not functional testing

---

## Running Playwright Tests

### Prerequisites

Install Playwright (already in devDependencies):

```bash
cd frontend
npm install -D @playwright/test
npx playwright install
```

### Run All Tests

```bash
cd frontend && npx playwright test
```

This will:
1. Start the web mode dev server automatically
2. Run all tests in `frontend/tests/visual/`
3. Generate HTML report

### Run Specific Test

```bash
cd frontend && npx playwright test kanban
```

### Interactive Mode

```bash
cd frontend && npx playwright test --ui
```

Opens the Playwright Test UI for interactive debugging.

### Debug Mode

```bash
cd frontend && npx playwright test --debug
```

Runs tests with step-by-step debugging.

---

## Visual Regression Testing

### How It Works

1. Tests take screenshots of the app in specific states
2. Screenshots are compared against baseline images in `frontend/tests/visual/snapshots/`
3. Differences above threshold (1% pixel diff) fail the test

### Update Baseline Screenshots

When UI intentionally changes:

```bash
cd frontend && npx playwright test --update-snapshots
```

This regenerates all snapshot files. Review the changes before committing.

### Snapshot Location

Snapshots are stored in:
```
frontend/tests/visual/snapshots/
└── kanban-board.png          # Kanban board snapshot
└── [test-name]-[browser].png # Other snapshots
```

### Threshold Configuration

Configured in `frontend/playwright.config.ts`:

```typescript
expect: {
  toHaveScreenshot: {
    maxDiffPixelRatio: 0.01, // 1% tolerance
  },
},
```

---

## Test Organization

### Directory Structure

Tests use a **modular Page Object Model (POM)** architecture:

```
frontend/tests/
├── visual/
│   ├── views/                    # View-specific specs
│   │   ├── kanban/
│   │   │   └── kanban.spec.ts           # Core layout tests (≤200 LOC)
│   │   ├── ideation/
│   │   │   └── ideation.spec.ts
│   │   └── activity/
│   │       └── activity.spec.ts
│   ├── modals/                   # Modal-specific tests
│   │   └── task-detail.spec.ts
│   ├── states/                   # Edge case specs
│   │   └── empty-states.spec.ts
│   └── snapshots/                # Baseline screenshots (auto-generated)
│
├── pages/                        # Page Object Model
│   ├── base.page.ts              # Shared navigation, waits
│   ├── kanban.page.ts            # Kanban-specific selectors/actions
│   └── modals/
│       └── task-detail.page.ts
│
├── fixtures/                     # Shared test data
│   └── setup.fixtures.ts         # Common beforeEach setup
│
└── helpers/                      # Utility functions
    └── wait.helpers.ts           # Custom wait conditions
```

### Page Object Model (POM)

All selectors must be in page objects, **never raw selectors in spec files**.

```typescript
// frontend/tests/pages/kanban.page.ts
import { Page, Locator } from "@playwright/test";
import { BasePage } from "./base.page";

export class KanbanPage extends BasePage {
  readonly taskCard: (id: string) => Locator;
  readonly column: (status: string) => Locator;
  readonly searchInput: Locator;

  constructor(page: Page) {
    super(page);
    this.taskCard = (id) => page.locator(`[data-testid="task-card-${id}"]`);
    this.column = (status) => page.locator(`[data-testid="column-${status}"]`);
    this.searchInput = page.locator('[data-testid="task-search"]');
  }

  async searchTasks(query: string) {
    await this.searchInput.fill(query);
    await this.page.keyboard.press("Enter");
  }
}
```

### Spec File Pattern

```typescript
// frontend/tests/visual/views/kanban/kanban.spec.ts
import { test, expect } from "@playwright/test";
import { KanbanPage } from "../../../pages/kanban.page";
import { setupKanban } from "../../../fixtures/setup.fixtures";

test.describe("Kanban Board", () => {
  let kanban: KanbanPage;

  test.beforeEach(async ({ page }) => {
    kanban = new KanbanPage(page);
    await setupKanban(page);  // Shared setup
  });

  test("renders board layout", async () => {
    await expect(kanban.column("todo")).toBeVisible();
    await expect(kanban.column("in_progress")).toBeVisible();
  });

  test("matches snapshot", async ({ page }) => {
    await kanban.waitForAnimations();
    await expect(page).toHaveScreenshot("kanban-board.png");
  });
});
```

### File Size Limits

| File Type | Max Lines | Refactor At | Action |
|-----------|-----------|-------------|--------|
| Spec file | 200 | 150 | Split by feature area |
| Page Object | 150 | 100 | Extract to sub-pages |
| Fixtures | 100 | 80 | Split by domain |
| Helpers | 50 | 40 | Extract to utilities |

### Split Triggers

| Condition | Action |
|-----------|--------|
| Spec file > 150 LOC | Split by feature area (e.g., `kanban-cards.spec.ts`) |
| > 10 tests in one file | Split by test type (visual/interaction/state) |
| Page object > 100 LOC | Extract to sub-page objects |
| Same selector in 3+ files | Move to page object |
| Same setup in 3+ files | Extract to fixture |

### Naming Conventions

| Type | Pattern | Example |
|------|---------|---------|
| Spec file | `{feature}.spec.ts` | `kanban.spec.ts` |
| Spec subset | `{feature}-{subset}.spec.ts` | `kanban-cards.spec.ts` |
| Page object | `{feature}.page.ts` | `kanban.page.ts` |
| Fixture | `{domain}.fixtures.ts` | `tasks.fixtures.ts` |
| Helper | `{purpose}.helpers.ts` | `wait.helpers.ts` |

### Code Quality Checklist

Before committing a new spec:
- [ ] File ≤ 200 LOC
- [ ] Uses Page Object for selectors (no raw `data-testid` in spec)
- [ ] Shared setup extracted to fixture
- [ ] Test names are descriptive (not "test 1", "test 2")
- [ ] One assertion focus per test
- [ ] Snapshot name matches test purpose

---

## Writing New Tests

### Basic Workflow

1. **Create page object** (if new feature): `frontend/tests/pages/{feature}.page.ts`
2. **Write spec** using page object pattern
3. **Generate baseline**: `npx playwright test [spec] --update-snapshots`
4. **Verify test passes**: `npx playwright test [spec]`

### Testing Tips

1. **Use data-testid attributes** - Add `data-testid="..."` to components for reliable selection
2. **Wait for hydration** - The app header is a good indicator: `[data-testid="app-header"]`
3. **Allow animation time** - Use `waitForTimeout(500)` before screenshots
4. **Test specific elements** - Use `.locator()` for targeted assertions

---

## Architecture Overview

### Environment Detection

`frontend/src/lib/tauri-detection.ts`:

```typescript
export function isWebMode(): boolean {
  return typeof window !== "undefined" && !window.__TAURI_INTERNALS__;
}
```

### API Switching

The API layer automatically switches based on environment:

- **Tauri mode**: Real API calls via `invoke()`
- **Web mode**: Mock API from `frontend/src/api-mock/`

### Event Bus

`frontend/src/lib/event-bus.ts` provides:

- **TauriEventBus**: Real Tauri `listen()`/`emit()` in native mode
- **MockEventBus**: In-memory event emitter in web mode

The `EventProvider` component in `frontend/src/providers/EventProvider.tsx` automatically selects the appropriate bus.

### Plugin Mocks

Vite aliases Tauri plugins to mocks in `frontend/src/mocks/` when in web mode:

| Plugin | Mock Location |
|--------|---------------|
| `@tauri-apps/plugin-dialog` | `frontend/src/mocks/tauri-plugin-dialog.ts` |
| `@tauri-apps/plugin-fs` | `frontend/src/mocks/tauri-plugin-fs.ts` |
| `@tauri-apps/plugin-process` | `frontend/src/mocks/tauri-plugin-process.ts` |
| `@tauri-apps/plugin-updater` | `frontend/src/mocks/tauri-plugin-updater.ts` |
| `@tauri-apps/plugin-global-shortcut` | `frontend/src/mocks/tauri-plugin-global-shortcut.ts` |

The updater mock returns no update by default. To preview the update UI in web mode, open
`/?mockUpdate=available` or set `localStorage.ralphx-mock-update=available`.

---

## Troubleshooting

### Dev Server Won't Start

**Port already in use:**
```bash
# Kill process on port 5173
lsof -ti:5173 | xargs kill -9

# Retry
cd frontend && npm run dev:web
```

### Tests Fail to Start

**Playwright browsers not installed:**
```bash
cd frontend && npx playwright install
```

**Web server timeout:**
- Increase timeout in `frontend/playwright.config.ts` (default: 120s)
- Check if `cd frontend && npm run dev:web` works manually

### Screenshots Don't Match

**Font rendering differences:**
- Different OS/browser versions render fonts slightly differently
- Use `maxDiffPixelRatio` to allow small differences

**Animation timing:**
- Add `await page.waitForTimeout(500)` before screenshot
- Ensure CSS animations have completed

**Dynamic content:**
- Mock data is deterministic but timestamps change
- Use `seededRandom` patterns for consistent mock data

### Mock Data Issues

**Missing mock implementation:**
- Check `frontend/src/api-mock/` for the API function
- Add mock implementation if missing

**Type errors with mocks:**
- Ensure mock return types match real API types
- Check `frontend/src/types/` for expected interfaces

### Console Errors in Web Mode

**"Failed to invoke command":**
- Expected when Tauri APIs are called directly without going through the mock layer
- Check if the code path is using the abstracted API

**"__TAURI_INTERNALS__ is undefined":**
- Expected in web mode, indicates detection is working
- Should not cause functional issues

---

## File Reference

| File | Purpose |
|------|---------|
| `frontend/playwright.config.ts` | Playwright configuration |
| `frontend/tests/visual/` | Visual regression spec files (views, modals, states) |
| `frontend/tests/visual/snapshots/` | Baseline screenshot images |
| `frontend/tests/pages/` | Page Object Model files |
| `frontend/tests/pages/base.page.ts` | Base page object with shared methods |
| `frontend/tests/fixtures/` | Shared test setup and data |
| `frontend/tests/helpers/` | Utility functions for tests |
| `frontend/src/api-mock/` | Mock API implementations |
| `frontend/src/mocks/` | Tauri plugin mocks |
| `frontend/src/lib/tauri-detection.ts` | Environment detection |
| `frontend/src/lib/event-bus.ts` | Event bus abstraction |
| `frontend/src/providers/EventProvider.tsx` | Event provider component |
| `frontend/vite.config.ts` | Vite config with web mode handling |
