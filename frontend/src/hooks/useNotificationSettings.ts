import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { z } from "zod";

import { typedInvoke } from "@/lib/tauri";

export interface NotificationSettings {
  desktop_enabled: boolean;
  desktop_only_when_unfocused: boolean;
  focused_toasts_enabled: boolean;
  desktop_agent_requests_enabled: boolean;
  desktop_agent_waiting_enabled: boolean;
  desktop_reviews_enabled: boolean;
  desktop_task_failures_enabled: boolean;
  desktop_automation_approvals_enabled: boolean;
  desktop_automation_run_completions_enabled: boolean;
  desktop_git_github_enabled: boolean;
  muted_project_ids: string[];
}

const notificationBoolean = (fallback: boolean) =>
  z.boolean().nullish().transform((value) => value ?? fallback);

/** Tolerates nullable persisted fields while retaining a typed settings contract. */
export const NotificationSettingsSchema = z.object({
  desktop_enabled: notificationBoolean(true),
  desktop_only_when_unfocused: notificationBoolean(true),
  focused_toasts_enabled: notificationBoolean(true),
  desktop_agent_requests_enabled: notificationBoolean(true),
  desktop_agent_waiting_enabled: notificationBoolean(true),
  desktop_reviews_enabled: notificationBoolean(true),
  desktop_task_failures_enabled: notificationBoolean(true),
  desktop_automation_approvals_enabled: notificationBoolean(true),
  desktop_automation_run_completions_enabled: notificationBoolean(false),
  desktop_git_github_enabled: notificationBoolean(true),
  muted_project_ids: z.array(z.string()).nullish().transform((value) => value ?? []),
});

export interface UpdateNotificationSettingsInput {
  desktopEnabled?: boolean;
  desktopOnlyWhenUnfocused?: boolean;
  focusedToastsEnabled?: boolean;
  desktopAgentRequestsEnabled?: boolean;
  desktopAgentWaitingEnabled?: boolean;
  desktopReviewsEnabled?: boolean;
  desktopTaskFailuresEnabled?: boolean;
  desktopAutomationApprovalsEnabled?: boolean;
  desktopAutomationRunCompletionsEnabled?: boolean;
  desktopGitGithubEnabled?: boolean;
  mutedProjectIds?: string[];
}

export const notificationSettingsKeys = {
  all: ["notification-settings"] as const,
};

export function useNotificationSettings() {
  return useQuery<NotificationSettings>({
    queryKey: notificationSettingsKeys.all,
    queryFn: () => typedInvoke("get_notification_settings", {}, NotificationSettingsSchema),
  });
}

export function useUpdateNotificationSettings() {
  const queryClient = useQueryClient();

  return useMutation<NotificationSettings, string, UpdateNotificationSettingsInput>({
    mutationFn: (input) =>
      typedInvoke("update_notification_settings", { input }, NotificationSettingsSchema),
    onSuccess: (settings) => {
      queryClient.setQueryData<NotificationSettings>(
        notificationSettingsKeys.all,
        settings,
      );
    },
  });
}
