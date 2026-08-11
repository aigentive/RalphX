export function preloadAutomationDetailView() {
  return import("./AutomationDetailView").then((module) => ({
    default: module.AutomationDetailView,
  }));
}
