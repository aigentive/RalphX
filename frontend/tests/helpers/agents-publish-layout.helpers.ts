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

export async function expectPrimaryActionSharesSummaryRow(
  publishPane: Locator,
  testId: string,
) {
  const summaryRow = publishPane.getByTestId("agents-publish-summary-row");
  const action = publishPane.getByTestId(testId);
  const [summaryRowBox, actionBox] = await Promise.all([
    summaryRow.boundingBox(),
    action.boundingBox(),
  ]);
  expect(summaryRowBox).not.toBeNull();
  expect(actionBox).not.toBeNull();

  const summaryCenterY = summaryRowBox!.y + summaryRowBox!.height / 2;
  const actionCenterY = actionBox!.y + actionBox!.height / 2;
  expect(Math.abs(summaryCenterY - actionCenterY)).toBeLessThanOrEqual(2);
}

export async function expectSummarySpacingBalanced(publishPane: Locator) {
  const summaryRow = publishPane.getByTestId("agents-publish-summary-row");
  const tabs = publishPane.getByTestId("agents-publish-tabs");
  const [paneBox, summaryRowBox, tabsBox] = await Promise.all([
    publishPane.boundingBox(),
    summaryRow.boundingBox(),
    tabs.boundingBox(),
  ]);
  expect(paneBox).not.toBeNull();
  expect(summaryRowBox).not.toBeNull();
  expect(tabsBox).not.toBeNull();

  const topSpacing = summaryRowBox!.y - paneBox!.y;
  const bottomSpacing =
    tabsBox!.y - (summaryRowBox!.y + summaryRowBox!.height);
  expect(Math.abs(topSpacing - bottomSpacing)).toBeLessThanOrEqual(1);
}
