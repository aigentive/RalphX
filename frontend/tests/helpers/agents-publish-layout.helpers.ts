import { expect, type Locator, type Page } from "@playwright/test";

export async function expectNoPaneOverflow(publishPane: Locator) {
  const horizontalOverflow = await publishPane.evaluate(
    (element) => element.scrollWidth - element.clientWidth,
  );
  expect(horizontalOverflow).toBeLessThanOrEqual(2);
}

export async function expectPrimaryActionContained(
  page: Page,
  publishPane: Locator,
  testId: string,
) {
  const action = page.getByTestId(testId);
  await expect(action).toBeVisible();
  const [actionBox, paneBox] = await Promise.all([
    action.boundingBox(),
    publishPane.boundingBox(),
  ]);
  expect(actionBox).not.toBeNull();
  expect(paneBox).not.toBeNull();
  expect(actionBox!.x).toBeGreaterThanOrEqual(paneBox!.x - 1);
  expect(actionBox!.x + actionBox!.width).toBeLessThanOrEqual(
    paneBox!.x + paneBox!.width + 1,
  );
  const viewport = page.viewportSize();
  if (viewport) {
    expect(actionBox!.x + actionBox!.width).toBeLessThanOrEqual(
      viewport.width + 1,
    );
  }
}
