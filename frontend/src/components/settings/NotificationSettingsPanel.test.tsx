import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, isPermissionGrantedMock, requestPermissionMock, openUrlMock } =
  vi.hoisted(() => ({
    invokeMock: vi.fn(),
    isPermissionGrantedMock: vi.fn(),
    requestPermissionMock: vi.fn(),
    openUrlMock: vi.fn(),
  }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: isPermissionGrantedMock,
  requestPermission: requestPermissionMock,
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: openUrlMock }));

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
    expect(screen.getByLabelText("Agent requests (permissions & questions)")).toBeChecked();
    expect(screen.getByLabelText("Automation run completions")).not.toBeChecked();
    expect(screen.getByText(/badge and Needs-action list always stay on/i)).toBeInTheDocument();
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
