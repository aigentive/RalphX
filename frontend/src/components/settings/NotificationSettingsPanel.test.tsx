import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { projects } = vi.hoisted(() => ({
  projects: [
    { id: "project-1", name: "RalphX" },
    { id: "project-2", name: "Other project" },
  ],
}));

const {
  invokeMock,
  isPermissionGrantedMock,
  requestPermissionMock,
  openUrlMock,
  toastErrorMock,
} = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  isPermissionGrantedMock: vi.fn(),
  requestPermissionMock: vi.fn(),
  openUrlMock: vi.fn(),
  toastErrorMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: isPermissionGrantedMock,
  requestPermission: requestPermissionMock,
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: openUrlMock }));
vi.mock("sonner", () => ({ toast: { error: toastErrorMock } }));
vi.mock("@/hooks/useProjects", () => ({ useProjects: () => ({ data: projects }) }));

import { NotificationSettingsPanel } from "./NotificationSettingsPanel";

const defaults = {
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

function renderPanel() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <NotificationSettingsPanel />
    </QueryClientProvider>,
  );
}

describe("NotificationSettingsPanel", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    isPermissionGrantedMock.mockReset().mockResolvedValue(true);
    requestPermissionMock.mockReset().mockResolvedValue("granted");
    openUrlMock.mockReset();
    toastErrorMock.mockReset();
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_notification_settings") return Promise.resolve(defaults);
      if (command === "update_notification_settings") return Promise.resolve(defaults);
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    });
  });

  it("renders every settings row with the persisted defaults", async () => {
    renderPanel();

    await screen.findByLabelText("Enable desktop notifications");
    expect(screen.getByText("Notify me about")).toBeInTheDocument();
    expect(screen.getByText("Muted projects")).toBeInTheDocument();
    expect(screen.getByLabelText("RalphX")).not.toBeChecked();
    expect(screen.getByLabelText("Agent requests (permissions & questions)")).toBeChecked();
    expect(screen.getByLabelText("Automation run completions")).not.toBeChecked();
    expect(screen.getByText(/badge and Needs-action list always stay on/i)).toBeInTheDocument();
  });

  it("rechecks granted macOS permission when desktop notifications are enabled on mount", async () => {
    renderPanel();

    await screen.findByLabelText("Enable desktop notifications");
    await waitFor(() => expect(isPermissionGrantedMock).toHaveBeenCalledOnce());
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("warns when macOS permission was revoked while desktop notifications stayed enabled", async () => {
    isPermissionGrantedMock.mockResolvedValue(false);
    renderPanel();

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Desktop notification permission is denied",
    );
  });

  it("disables desktop children when the desktop master setting is off", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_notification_settings") {
        return Promise.resolve({ ...defaults, desktop_enabled: false });
      }
      return Promise.resolve(defaults);
    });
    renderPanel();

    await waitFor(() =>
      expect(screen.getByLabelText("Enable desktop notifications")).toHaveAttribute(
        "aria-checked",
        "false",
      ),
    );
    expect(screen.getByLabelText("Only when RalphX is in the background")).toBeDisabled();
    expect(screen.getByLabelText("Reviews & escalations")).toBeDisabled();
    expect(screen.getByLabelText("In-app toasts for actionable items")).not.toBeDisabled();
  });

  it("sends camel-case mutation payloads for category toggles", async () => {
    renderPanel();

    const toggle = await screen.findByLabelText("Reviews & escalations");
    fireEvent.click(toggle);

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("update_notification_settings", {
        input: { desktopReviewsEnabled: false },
      }),
    );
  });

  it("adds and removes projects from the muted-project setting", async () => {
    invokeMock.mockImplementation((command: string, args?: { input?: { mutedProjectIds?: string[] } }) => {
      if (command === "get_notification_settings") return Promise.resolve(defaults);
      if (command === "update_notification_settings") {
        return Promise.resolve({ ...defaults, muted_project_ids: args?.input?.mutedProjectIds ?? [] });
      }
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    });
    renderPanel();

    fireEvent.click(await screen.findByLabelText("RalphX"));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("update_notification_settings", {
        input: { mutedProjectIds: ["project-1"] },
      }),
    );

    fireEvent.click(await screen.findByLabelText("RalphX"));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenLastCalledWith("update_notification_settings", {
        input: { mutedProjectIds: [] },
      }),
    );
  });

  it("requests desktop permission on first enable and warns when macOS denies it", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_notification_settings") {
        return Promise.resolve({ ...defaults, desktop_enabled: false });
      }
      return Promise.resolve({ ...defaults, desktop_enabled: true });
    });
    isPermissionGrantedMock.mockResolvedValue(false);
    requestPermissionMock.mockResolvedValue("denied");
    renderPanel();

    await waitFor(() =>
      expect(screen.getByLabelText("Enable desktop notifications")).toHaveAttribute(
        "aria-checked",
        "false",
      ),
    );
    fireEvent.click(await screen.findByLabelText("Enable desktop notifications"));

    await waitFor(() => expect(requestPermissionMock).toHaveBeenCalledOnce());
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Desktop notification permission is denied",
    );
    fireEvent.click(screen.getByRole("button", { name: "System Settings…" }));
    expect(openUrlMock).toHaveBeenCalledWith(
      "x-apple.systempreferences:com.apple.Notifications-Settings.extension",
    );
  });
});

/**
 * The macOS permission probe is advisory and must not be able to eat the durable write.
 *
 * Before Phase 2, `plugin:notification|*` travelled to the host and rejected there, and the
 * rejection short-circuited `updateDesktopEnabled` BEFORE `update_notification_settings` — a
 * registered facade op that would have succeeded. The switch animated, nothing persisted, and
 * reopening the pane showed it off again. The `plugin:` prefix rule now keeps the probe local;
 * these cases pin the failure handling that has to hold regardless of why it rejects.
 */
describe("NotificationSettingsPanel — permission probe failures", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    isPermissionGrantedMock.mockReset().mockResolvedValue(true);
    requestPermissionMock.mockReset().mockResolvedValue("granted");
    openUrlMock.mockReset();
    toastErrorMock.mockReset();
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_notification_settings") return Promise.resolve(defaults);
      if (command === "update_notification_settings") return Promise.resolve(defaults);
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    });
  });

  it("still persists the setting when the permission probe rejects", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_notification_settings") {
        return Promise.resolve({ ...defaults, desktop_enabled: false });
      }
      return Promise.resolve({ ...defaults, desktop_enabled: true });
    });
    isPermissionGrantedMock.mockRejectedValue(new Error("probe unavailable"));

    renderPanel();
    await waitFor(() =>
      expect(screen.getByLabelText("Enable desktop notifications")).toHaveAttribute(
        "aria-checked",
        "false",
      ),
    );
    fireEvent.click(screen.getByLabelText("Enable desktop notifications"));

    // The whole defect: this write never happened.
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("update_notification_settings", {
        input: { desktopEnabled: true },
      }),
    );
  });

  it("does not claim macOS DENIED permission when the probe merely failed", async () => {
    isPermissionGrantedMock.mockRejectedValue(new Error("probe unavailable"));

    renderPanel();
    await screen.findByLabelText("Enable desktop notifications");
    await waitFor(() => expect(isPermissionGrantedMock).toHaveBeenCalled());

    // A failed read is not a denial: asserting an OS fact we never established would send the
    // user to System Settings to fix something that may not be broken.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("surfaces a failed write instead of discarding the rejected promise", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_notification_settings") return Promise.resolve(defaults);
      if (command === "update_notification_settings") {
        return Promise.reject(new Error("REMOTE_FORBIDDEN"));
      }
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    });

    renderPanel();
    fireEvent.click(await screen.findByLabelText("Enable desktop notifications"));

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith("Could not update desktop notifications."),
    );
  });

  it("keeps the mount-time probe from rejecting unhandled", async () => {
    const unhandled = vi.fn();
    process.on("unhandledRejection", unhandled);
    isPermissionGrantedMock.mockRejectedValue(new Error("probe unavailable"));

    try {
      renderPanel();
      await screen.findByLabelText("Enable desktop notifications");
      await waitFor(() => expect(isPermissionGrantedMock).toHaveBeenCalled());
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(unhandled).not.toHaveBeenCalled();
    } finally {
      process.off("unhandledRejection", unhandled);
    }
  });
});
