import { useNotificationSettings } from "./useNotificationSettings";

export interface NotificationPreferences {
  ready: boolean;
  focusedToastsEnabled: boolean;
  mutedProjectIds: string[];
}

/** Consumer seam for focused toast gating; fail closed until persisted settings hydrate. */
export function useNotificationPreferences(): NotificationPreferences {
  const { data: settings, isSuccess } = useNotificationSettings();
  return {
    ready: isSuccess,
    focusedToastsEnabled: settings?.focused_toasts_enabled ?? false,
    mutedProjectIds: settings?.muted_project_ids ?? [],
  };
}
