import type { PlanBundleBodyMode } from "./PlanBundleTabs";

export function planBundleTabId(
  idPrefix: string,
  mode: PlanBundleBodyMode,
) {
  return `${idPrefix}-tab-${mode}`;
}

export function planBundlePanelId(
  idPrefix: string,
  mode: PlanBundleBodyMode,
) {
  return `${idPrefix}-panel-${mode}`;
}
