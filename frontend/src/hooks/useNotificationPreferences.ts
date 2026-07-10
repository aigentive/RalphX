export interface NotificationPreferences {
  focusedToastsEnabled: boolean;
}

/** Settings seam; PR 9 replaces this default with persisted notification preferences. */
export function useNotificationPreferences(): NotificationPreferences {
  return { focusedToastsEnabled: true };
}
