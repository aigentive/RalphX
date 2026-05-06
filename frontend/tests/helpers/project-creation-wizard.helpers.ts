import { Page } from "@playwright/test";

/**
 * Trigger the ProjectCreationWizard modal in web mode
 *
 * Opens the welcome overlay and uses its create-project CTA. The v27 topbar no
 * longer renders ProjectSelector, so this follows the current user-facing path.
 */
export async function openProjectCreationWizard(page: Page): Promise<void> {
  await page.evaluate(() => {
    const uiStore = (window as Window & {
      __uiStore?: { getState(): { openWelcomeOverlay(): void } };
    }).__uiStore;
    uiStore?.getState().openWelcomeOverlay();
  });

  await page.waitForSelector('[data-testid="welcome-screen"]', { state: "visible" });
  await page.click('[data-testid="create-first-project-button"]');
  await page.waitForSelector('[data-testid="project-creation-wizard"]', { state: "visible" });
}

/**
 * Create test project data
 */
export function createTestProjectData() {
  return {
    localMode: {
      name: "Test Project Local",
      workingDirectory: "/Users/test/projects/test-project",
      gitMode: "local" as const,
    },
    worktreeMode: {
      name: "Test Project Worktree",
      workingDirectory: "/Users/test/projects/test-project",
      gitMode: "worktree" as const,
      baseBranch: "main",
      worktreeBranch: "ralphx/test-feature",
      worktreePath: "/Users/test/projects/test-project/.ralphx-worktree",
    },
  };
}
