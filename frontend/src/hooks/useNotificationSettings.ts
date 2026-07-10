import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

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
}

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
}

export const notificationSettingsKeys = {
  all: ["notification-settings"] as const,
};

export function useNotificationSettings() {
  return useQuery<NotificationSettings>({
    queryKey: notificationSettingsKeys.all,
    queryFn: () => invoke<NotificationSettings>("get_notification_settings"),
  });
}

export function useUpdateNotificationSettings() {
  const queryClient = useQueryClient();

  return useMutation<NotificationSettings, string, UpdateNotificationSettingsInput>({
    mutationFn: (input) =>
      invoke<NotificationSettings>("update_notification_settings", { input }),
    onSuccess: (settings) => {
      queryClient.setQueryData<NotificationSettings>(
        notificationSettingsKeys.all,
        settings,
      );
    },
  });
}
