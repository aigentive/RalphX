import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createElement } from "react";

import { ClickUpIntegrationSettingsPanel } from "./ClickUpIntegrationSettingsPanel";

const clickupHook = vi.hoisted(() => ({
  saveSettingsAsync: vi.fn(),
  validateAsync: vi.fn(),
  disconnectAsync: vi.fn(),
  state: {
    settings: {
      enabled: false,
      hasApiToken: false,
      workspaceId: null as string | null,
      validationStatus: "not_configured",
      taskSearchAvailable: false,
      lastValidatedAt: null as string | null,
      lastError: null as string | null,
      updatedAt: new Date(0).toISOString(),
    },
    isLoading: false,
    isError: false,
    error: null as Error | null,
    connected: false,
    workspaces: [] as { id: string; name: string }[],
    isLoadingWorkspaces: false,
    workspacesError: null as Error | null,
    isSavingSettings: false,
    isValidating: false,
    isDisconnecting: false,
    saveSettingsError: null as Error | null,
    validateError: null as Error | null,
    disconnectError: null as Error | null,
  },
}));

vi.mock("@/hooks/useClickUpIntegration", () => ({
  useClickUpIntegration: () => ({
    ...clickupHook.state,
    saveSettingsAsync: clickupHook.saveSettingsAsync,
    validateAsync: clickupHook.validateAsync,
    disconnectAsync: clickupHook.disconnectAsync,
  }),
}));

function renderPanel() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return {
    queryClient,
    ...render(
      createElement(
        QueryClientProvider,
        { client: queryClient },
        createElement(ClickUpIntegrationSettingsPanel),
      ),
    ),
  };
}

describe("ClickUpIntegrationSettingsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    clickupHook.state.settings = {
      enabled: false,
      hasApiToken: false,
      workspaceId: null,
      validationStatus: "not_configured",
      taskSearchAvailable: false,
      lastValidatedAt: null,
      lastError: null,
      updatedAt: new Date(0).toISOString(),
    };
    clickupHook.state.isLoading = false;
    clickupHook.state.isError = false;
    clickupHook.state.error = null;
    clickupHook.state.connected = false;
    clickupHook.state.workspaces = [];
    clickupHook.state.isLoadingWorkspaces = false;
    clickupHook.state.workspacesError = null;
    clickupHook.state.isSavingSettings = false;
    clickupHook.state.isValidating = false;
    clickupHook.state.isDisconnecting = false;
    clickupHook.state.saveSettingsError = null;
    clickupHook.state.validateError = null;
    clickupHook.state.disconnectError = null;
    clickupHook.disconnectAsync.mockResolvedValue({
      enabled: false,
      hasApiToken: false,
      workspaceId: null,
      validationStatus: "not_configured",
      taskSearchAvailable: false,
      updatedAt: new Date(0).toISOString(),
    });
    clickupHook.saveSettingsAsync.mockResolvedValue({
      enabled: false,
      hasApiToken: true,
      workspaceId: null,
      validationStatus: "pending",
      taskSearchAvailable: false,
      updatedAt: new Date(0).toISOString(),
    });
    clickupHook.validateAsync.mockResolvedValue({
      enabled: true,
      hasApiToken: true,
      workspaceId: null,
      validationStatus: "valid",
      taskSearchAvailable: true,
      updatedAt: new Date(0).toISOString(),
    });
  });

  it("shows ClickUp task reference configuration status", () => {
    renderPanel();

    expect(screen.getByText("Task references not ready")).toBeInTheDocument();
  });

  it("saves and validates the API token", async () => {
    const user = userEvent.setup();
    clickupHook.state.settings.hasApiToken = true;
    renderPanel();

    await user.type(screen.getByLabelText("API token"), "pk_clickup_token");
    await user.click(screen.getByRole("button", { name: /Save API token/ }));

    expect(clickupHook.saveSettingsAsync).toHaveBeenCalledWith({
      apiToken: "pk_clickup_token",
    });
    expect(clickupHook.validateAsync).toHaveBeenCalled();
  });

  it("rejects an empty API token before invoking the backend", async () => {
    const user = userEvent.setup();
    renderPanel();

    await user.click(screen.getByRole("button", { name: /Save API token/ }));

    expect(
      await screen.findByText("ClickUp API token cannot be empty"),
    ).toBeInTheDocument();
    expect(clickupHook.saveSettingsAsync).not.toHaveBeenCalled();
  });

  it("shows backend validation failures after saving a token", async () => {
    const user = userEvent.setup();
    clickupHook.state.settings.hasApiToken = true;
    clickupHook.validateAsync.mockResolvedValue({
      enabled: false,
      hasApiToken: true,
      workspaceId: null,
      validationStatus: "invalid",
      taskSearchAvailable: false,
      lastError: "ClickUp returned HTTP 401",
      updatedAt: new Date(0).toISOString(),
    });

    renderPanel();

    await user.type(screen.getByLabelText("API token"), "pk_clickup_token");
    await user.click(screen.getByRole("button", { name: /Save API token/ }));

    expect(
      await screen.findByText("ClickUp returned HTTP 401"),
    ).toBeInTheDocument();
  });

  it("shows string errors from the validate command", async () => {
    const user = userEvent.setup();
    clickupHook.state.settings.hasApiToken = true;
    clickupHook.validateAsync.mockRejectedValue(
      "ClickUp API token is missing from secure storage",
    );

    renderPanel();

    await user.click(screen.getByRole("button", { name: "Validate" }));

    expect(
      await screen.findByText(
        "ClickUp API token is missing from secure storage",
      ),
    ).toBeInTheDocument();
  });

  it("does not offer disconnect or a workspace selector when nothing is configured", () => {
    renderPanel();

    expect(screen.getByTestId("integration-status-banner")).toHaveAttribute(
      "data-connected",
      "false",
    );
    expect(
      screen.queryByRole("button", { name: "Disconnect" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Workspace")).not.toBeInTheDocument();
  });

  it("paints the status banner green and offers a workspace selector when connected", () => {
    clickupHook.state.settings = {
      enabled: true,
      hasApiToken: true,
      workspaceId: "team-1",
      validationStatus: "valid",
      taskSearchAvailable: true,
      lastValidatedAt: new Date(0).toISOString(),
      lastError: null,
      updatedAt: new Date(0).toISOString(),
    };
    clickupHook.state.connected = true;
    clickupHook.state.workspaces = [
      { id: "team-1", name: "Acme" },
      { id: "team-2", name: "Globex" },
    ];

    renderPanel();

    expect(screen.getByText("Task references enabled")).toBeInTheDocument();
    expect(screen.getByTestId("integration-status-banner")).toHaveAttribute(
      "data-connected",
      "true",
    );
    expect(
      screen.getByRole("button", { name: "Disconnect" }),
    ).toBeInTheDocument();
    const workspaceSelect = screen.getByLabelText<HTMLSelectElement>(
      "Workspace",
    );
    expect(workspaceSelect).toBeInTheDocument();
    expect(workspaceSelect.value).toBe("team-1");
    expect(screen.getByRole("option", { name: "Globex" })).toBeInTheDocument();
  });

  it("persists the selected workspace via saveSettings", async () => {
    const user = userEvent.setup();
    clickupHook.state.settings = {
      enabled: true,
      hasApiToken: true,
      workspaceId: "team-1",
      validationStatus: "valid",
      taskSearchAvailable: true,
      lastValidatedAt: new Date(0).toISOString(),
      lastError: null,
      updatedAt: new Date(0).toISOString(),
    };
    clickupHook.state.connected = true;
    clickupHook.state.workspaces = [
      { id: "team-1", name: "Acme" },
      { id: "team-2", name: "Globex" },
    ];
    clickupHook.saveSettingsAsync.mockResolvedValue({
      ...clickupHook.state.settings,
      workspaceId: "team-2",
    });

    renderPanel();

    await user.selectOptions(screen.getByLabelText("Workspace"), "team-2");

    expect(clickupHook.saveSettingsAsync).toHaveBeenCalledWith({
      workspaceId: "team-2",
    });
  });

  it("clears the connection after confirming disconnect", async () => {
    const user = userEvent.setup();
    clickupHook.state.settings.hasApiToken = true;
    renderPanel();

    await user.click(screen.getByRole("button", { name: "Disconnect" }));
    // First click only reveals the confirmation step; nothing cleared yet.
    expect(clickupHook.disconnectAsync).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Confirm disconnect" }));

    expect(clickupHook.disconnectAsync).toHaveBeenCalledTimes(1);
  });
});
