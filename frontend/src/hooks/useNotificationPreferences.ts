import { useNotificationSettings } from "./useNotificationSettings";

export interface NotificationPreferences {
  focusedToastsEnabled: boolean;
}

/** Consumer seam for focused toast gating; defaults true while settings hydrate. */
export function useNotificationPreferences(): NotificationPreferences {
  const { data: settings } = useNotificationSettings();
  return { focusedToastsEnabled: settings?.focused_toasts_enabled ?? true };
}
