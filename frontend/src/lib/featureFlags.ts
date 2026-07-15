import type { FeatureFlags } from "@/types/feature-flags";

export function applyFeatureFlagOverrides(flags: FeatureFlags): FeatureFlags {
  return flags;
}

export function isViewEnabled(view: string, flags: FeatureFlags): boolean {
  switch (view) {
    case "activity":
      return flags.activityPage;
    case "extensibility":
      return flags.extensibilityPage;
    case "ideation":
      return flags.ideationPage;
    case "automations":
      return flags.automationsPage;
    case "ticketing":
      return true;
    default:
      return true;
  }
}
