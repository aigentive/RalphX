import { useEffect, useState, type ReactNode } from "react";
import { Info, TriangleAlert } from "lucide-react";
import { toast } from "sonner";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";

import {
  type NotificationSettings,
  useNotificationSettings,
  useUpdateNotificationSettings,
  type UpdateNotificationSettingsInput,
} from "@/hooks/useNotificationSettings";
import { useProjects } from "@/hooks/useProjects";

import { SettingsSection, ToggleSettingRow } from "./SettingsView.shared";

const DEFAULT_NOTIFICATION_SETTINGS: NotificationSettings = {
  desktop_enabled: true,
  desktop_only_when_unfocused: true,
  focused_toasts_enabled: true,
  desktop_agent_requests_enabled: true,
  desktop_agent_waiting_enabled: true,
  desktop_reviews_enabled: true,
  desktop_task_failures_enabled: true,
  desktop_automation_approvals_enabled: true,
  desktop_automation_run_completions_enabled: false,
  desktop_git_github_enabled: true,
  muted_project_ids: [],
};

interface DesktopCategoryRow {
  id: string;
  label: string;
  description: string;
  field: keyof Pick<
    NotificationSettings,
    | "desktop_agent_requests_enabled"
    | "desktop_agent_waiting_enabled"
    | "desktop_reviews_enabled"
    | "desktop_task_failures_enabled"
    | "desktop_automation_approvals_enabled"
    | "desktop_automation_run_completions_enabled"
    | "desktop_git_github_enabled"
  >;
  input: keyof Pick<
    UpdateNotificationSettingsInput,
    | "desktopAgentRequestsEnabled"
    | "desktopAgentWaitingEnabled"
    | "desktopReviewsEnabled"
    | "desktopTaskFailuresEnabled"
    | "desktopAutomationApprovalsEnabled"
    | "desktopAutomationRunCompletionsEnabled"
    | "desktopGitGithubEnabled"
  >;
}

const DESKTOP_CATEGORY_ROWS: readonly DesktopCategoryRow[] = [
  {
    id: "notification-agent-requests",
    label: "Agent requests (permissions & questions)",
    description: "Permission requests and agent questions.",
    field: "desktop_agent_requests_enabled",
    input: "desktopAgentRequestsEnabled",
  },
  {
    id: "notification-agent-waiting",
    label: "Agent waiting for your reply",
    description: "Your turn notifications when an agent needs you.",
    field: "desktop_agent_waiting_enabled",
    input: "desktopAgentWaitingEnabled",
  },
  {
    id: "notification-reviews",
    label: "Reviews & escalations",
    description: "Review decisions, escalations, and plan approvals.",
    field: "desktop_reviews_enabled",
    input: "desktopReviewsEnabled",
  },
  {
    id: "notification-task-failures",
    label: "Task failures & merge conflicts",
    description: "Task failures, QA failures, and merge recovery needs.",
    field: "desktop_task_failures_enabled",
    input: "desktopTaskFailuresEnabled",
  },
  {
    id: "notification-automation-approvals",
    label: "Automation approvals & pauses",
    description: "Automation plan approvals, pauses, and failed runs.",
    field: "desktop_automation_approvals_enabled",
    input: "desktopAutomationApprovalsEnabled",
  },
  {
    id: "notification-automation-completions",
    label: "Automation run completions",
    description: "Completed automation runs.",
    field: "desktop_automation_run_completions_enabled",
    input: "desktopAutomationRunCompletionsEnabled",
  },
  {
    id: "notification-git-github",
    label: "Git & GitHub authentication",
    description: "Git, GitHub, and pull request actions needing attention.",
    field: "desktop_git_github_enabled",
    input: "desktopGitGithubEnabled",
  },
];

function InlineNotice({
  tone,
  children,
}: {
  tone: "info" | "warn";
  children: ReactNode;
}) {
  const isWarning = tone === "warn";
  const Icon = isWarning ? TriangleAlert : Info;
  return (
    <div
      role={isWarning ? "alert" : "note"}
      className={
        isWarning
          ? "flex items-start gap-2 rounded-md border border-[var(--notice-warn-border)] bg-[var(--notice-warn-bg)] px-3 py-2 text-[0.6875rem] leading-relaxed text-[var(--notice-warn-text)]"
          : "flex items-start gap-2 rounded-md border border-[var(--notice-info-border)] bg-[var(--notice-info-bg)] px-3 py-2 text-[0.6875rem] leading-relaxed text-[var(--notice-info-text)]"
      }
    >
      <Icon
        className={
          isWarning
            ? "mt-0.5 h-3.5 w-3.5 shrink-0 text-[var(--notice-warn-icon)]"
            : "mt-0.5 h-3.5 w-3.5 shrink-0 text-[var(--notice-info-icon)]"
        }
        aria-hidden="true"
      />
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}

export function NotificationSettingsPanel() {
  const { data } = useNotificationSettings();
  const { data: projects = [] } = useProjects();
  const {
    mutate: updateSettings,
    mutateAsync: updateSettingsAsync,
    isPending,
  } = useUpdateNotificationSettings();
  const [permissionDenied, setPermissionDenied] = useState(false);
  const settings = data ?? DEFAULT_NOTIFICATION_SETTINGS;
  const desktopChildrenDisabled = !settings.desktop_enabled || isPending;

  /**
   * The macOS permission probe is ADVISORY, and it is local.
   *
   * `plugin:notification|*` is pinned to this device by the `plugin:` prefix rule in
   * `lib/remote/local-only-commands.ts`, so under a remote environment it now reads THIS
   * Mac's grant instead of rejecting at the host. Two failure rules follow from that:
   *
   * - a failed probe must never gate the durable write. `update_notification_settings` is a
   *   registered facade op; letting the probe short-circuit it is what made this toggle a
   *   silent no-op — the switch animated, nothing persisted, and reopening the pane showed
   *   it off again.
   * - a failed READ is not a denial. Rendering "permission is denied in macOS" off an error
   *   would assert an OS fact we did not establish, so unknown stays quiet.
   */
  const requestDesktopPermissionIfNeeded = async () => {
    try {
      if (await isPermissionGranted()) {
        setPermissionDenied(false);
        return;
      }

      const permission = await requestPermission();
      setPermissionDenied(permission !== "granted");
    } catch (error) {
      console.warn("Could not read the macOS notification permission", error);
      setPermissionDenied(false);
    }
  };

  useEffect(() => {
    if (!data?.desktop_enabled) return;

    void isPermissionGranted()
      .then((granted) => {
        setPermissionDenied(!granted);
      })
      // Missing before: this rejected on every mount of the pane under a remote environment,
      // producing an unhandled rejection and no user-visible signal at all.
      .catch((error: unknown) => {
        console.warn("Could not read the macOS notification permission", error);
      });
  }, [data?.desktop_enabled]);

  const updateDesktopEnabled = async (enabled: boolean) => {
    if (enabled) {
      await requestDesktopPermissionIfNeeded();
    }
    // `mutateAsync`, so a rejected write reaches the caller instead of being swallowed by
    // `mutate`'s fire-and-forget contract.
    await updateSettingsAsync({ desktopEnabled: enabled });
  };

  const handleDesktopEnabledChange = (enabled: boolean) => {
    // Deliberately not `void`-discarded: a rejected write used to disappear, leaving the
    // switch to snap back with no explanation.
    updateDesktopEnabled(enabled).catch((error: unknown) => {
      console.error("Failed to update the desktop-notification setting", error);
      toast.error("Could not update desktop notifications.");
    });
  };

  const updateProjectMuted = (projectId: string, muted: boolean) => {
    const mutedProjectIds = muted
      ? [...new Set([...settings.muted_project_ids, projectId])]
      : settings.muted_project_ids.filter((id) => id !== projectId);
    updateSettings({ mutedProjectIds });
  };

  return (
    <SettingsSection>
      <ToggleSettingRow
        id="notification-desktop-enabled"
        label="Enable desktop notifications"
        description="Native macOS alerts when RalphX needs you."
        checked={settings.desktop_enabled}
        disabled={isPending}
        onChange={handleDesktopEnabledChange}
      />
      <ToggleSettingRow
        id="notification-desktop-only-unfocused"
        label="Only when RalphX is in the background"
        description="Alerts are suppressed while the app window is focused."
        checked={settings.desktop_only_when_unfocused}
        disabled={desktopChildrenDisabled}
        isSubSetting={true}
        onChange={(enabled) => updateSettings({ desktopOnlyWhenUnfocused: enabled })}
      />
      <ToggleSettingRow
        id="notification-focused-toasts"
        label="In-app toasts for actionable items"
        description="Show a toast while RalphX is focused and the notification center is closed."
        checked={settings.focused_toasts_enabled}
        // In-app toasts are independent of the desktop-notification master switch.
        disabled={isPending}
        onChange={(enabled) => updateSettings({ focusedToastsEnabled: enabled })}
      />

      <div className="pt-2 text-[11px] font-semibold uppercase tracking-[0.08em] text-[var(--text-secondary)]">
        Notify me about
      </div>
      {DESKTOP_CATEGORY_ROWS.map((row) => (
        <ToggleSettingRow
          key={row.id}
          id={row.id}
          label={row.label}
          description={row.description}
          checked={settings[row.field]}
          disabled={desktopChildrenDisabled}
          onChange={(enabled) => updateSettings({ [row.input]: enabled })}
        />
      ))}

      {projects.length > 0 ? (
        <>
          <div className="pt-2 text-[11px] font-semibold uppercase tracking-[0.08em] text-[var(--text-secondary)]">
            Muted projects
          </div>
          {projects.map((project) => (
            <ToggleSettingRow
              key={project.id}
              id={`notification-muted-project-${project.id}`}
              label={project.name}
              description="Suppress desktop alerts and in-app toasts for this project."
              checked={settings.muted_project_ids.includes(project.id)}
              disabled={isPending}
              onChange={(muted) => updateProjectMuted(project.id, muted)}
            />
          ))}
        </>
      ) : null}

      <InlineNotice tone="info">
        The in-app badge and Needs-action list always stay on — these toggles
        only control desktop alerts and toasts.
      </InlineNotice>
      {permissionDenied ? (
        <InlineNotice tone="warn">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span>Desktop notification permission is denied in macOS.</span>
            <button
              type="button"
              className="rounded-md border border-[var(--border-default)] bg-[var(--bg-elevated)] px-2.5 py-1 text-xs font-medium text-[var(--text-primary)] transition-colors hover:border-[var(--accent-primary)] hover:text-[var(--accent-primary)]"
              onClick={() =>
                void openUrl(
                  "x-apple.systempreferences:com.apple.Notifications-Settings.extension",
                )
              }
            >
              System Settings…
            </button>
          </div>
        </InlineNotice>
      ) : null}
    </SettingsSection>
  );
}
