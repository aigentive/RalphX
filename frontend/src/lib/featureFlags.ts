import type { AppView } from "@/types/app-view";
import type { FeatureFlags } from "@/types/feature-flags";

export function applyFeatureFlagOverrides(flags: FeatureFlags): FeatureFlags {
  return flags;
}

export function isViewEnabled(view: AppView, flags: FeatureFlags): boolean {
  switch (view) {
    case "activity":
      return flags.activityPage;
    case "extensibility":
      return flags.extensibilityPage;
    case "automations":
      return flags.automationsPage;
    case "ticketing":
      return true;
    default:
      return true;
  }
}
