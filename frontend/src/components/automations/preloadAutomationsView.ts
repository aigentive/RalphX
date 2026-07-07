export function preloadAutomationsView() {
  return import("./AutomationsView").then((module) => ({
    default: module.AutomationsView,
  }));
}
