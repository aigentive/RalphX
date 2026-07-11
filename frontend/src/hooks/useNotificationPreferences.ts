import { useNotificationSettings } from "./useNotificationSettings";

export interface NotificationPreferences {
  focusedToastsEnabled: boolean;
  mutedProjectIds: string[];
}

/** Consumer seam for focused toast gating; defaults true while settings hydrate. */
export function useNotificationPreferences(): NotificationPreferences {
  const { data: settings } = useNotificationSettings();
  return {
    focusedToastsEnabled: settings?.focused_toasts_enabled ?? true,
    mutedProjectIds: settings?.muted_project_ids ?? [],
  };
}
