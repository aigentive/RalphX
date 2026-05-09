import { Page } from "@playwright/test";

/**
 * Trigger the ProjectCreationWizard modal in web mode
 *
 * Uses the Agents sidebar add-project control. The welcome surface is now an
 * onboarding flow, so visual tests should open this modal through the app-level
 * project creation action instead of depending on a specific welcome step.
 */
export async function openProjectCreationWizard(page: Page): Promise<void> {
  const addProjectButton = page.locator('[data-testid="agents-add-project"]');

  if (!(await addProjectButton.isVisible())) {
    const agentsNavButton = page.locator('[data-testid="nav-agents"]');
    if (await agentsNavButton.isVisible()) {
      await agentsNavButton.click();
    }
  }

  if (await addProjectButton.isVisible()) {
    await addProjectButton.click();
  } else {
    await page.keyboard.press("Meta+Shift+N");
  }

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
