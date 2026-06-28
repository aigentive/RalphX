/**
 * Test Helpers: TaskPickerDialog
 *
 * Utilities for triggering and interacting with TaskPickerDialog in visual tests.
 */

import { Page } from "@playwright/test";
import { setupIdeation } from "../fixtures/setup.fixtures";

/**
 * Opens TaskPickerDialog by navigating to ideation view and clicking trigger
 */
export async function openTaskPickerDialog(page: Page): Promise<void> {
  await setupIdeation(page);

  // Wait for ideation view to be ready - look for the "Seed from Draft Task" button
  await page.waitForSelector('button:has-text("Seed from Draft Task")', { timeout: 5000 });

  // Click "Seed from Draft Task" button to open TaskPickerDialog
  const seedButton = page.getByRole("button", { name: "Seed from Draft Task" });
  await seedButton.click();

  // Wait for dialog to appear
  await page.waitForSelector('[role="dialog"]', { timeout: 3000 });
}

/**
 * Opens TaskPickerDialog directly via window manipulation (for isolated testing)
 */
export async function openTaskPickerDialogDirect(page: Page): Promise<void> {
  await setupIdeation(page);

  // Trigger dialog via state manipulation (if exposed to window)
  await page.evaluate(() => {
    // This requires the component to be controlled by a state exposed to window
    // If not available, we'll need to use the natural trigger method above
    const event = new CustomEvent("open-task-picker");
    window.dispatchEvent(event);
  });
}
