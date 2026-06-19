import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { LinearIntegrationSettingsPanel } from "./LinearIntegrationSettingsPanel";

const linearHook = vi.hoisted(() => ({
  saveSettingsAsync: vi.fn(),
  validateAsync: vi.fn(),
  disconnectAsync: vi.fn(),
  state: {
    settings: {
      enabled: false,
      hasApiToken: false,
      validationStatus: "not_configured",
      issueSearchAvailable: false,
      lastValidatedAt: null,
      lastError: null,
      updatedAt: new Date(0).toISOString(),
    },
    isLoading: false,
    isError: false,
    error: null as Error | null,
    isSavingSettings: false,
    isValidating: false,
    isDisconnecting: false,
    saveSettingsError: null as Error | null,
    validateError: null as Error | null,
    disconnectError: null as Error | null,
  },
}));

vi.mock("@/hooks/useLinearIntegration", () => ({
  useLinearIntegration: () => ({
    ...linearHook.state,
    saveSettingsAsync: linearHook.saveSettingsAsync,
    validateAsync: linearHook.validateAsync,
    disconnectAsync: linearHook.disconnectAsync,
  }),
}));

describe("LinearIntegrationSettingsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    linearHook.state.settings = {
      enabled: false,
      hasApiToken: false,
      validationStatus: "not_configured",
      issueSearchAvailable: false,
      lastValidatedAt: null,
      lastError: null,
      updatedAt: new Date(0).toISOString(),
    };
    linearHook.state.isLoading = false;
    linearHook.state.isError = false;
    linearHook.state.error = null;
    linearHook.state.isSavingSettings = false;
    linearHook.state.isValidating = false;
    linearHook.state.isDisconnecting = false;
    linearHook.state.saveSettingsError = null;
    linearHook.state.validateError = null;
    linearHook.state.disconnectError = null;
    linearHook.disconnectAsync.mockResolvedValue({
      enabled: false,
      hasApiToken: false,
      validationStatus: "not_configured",
      issueSearchAvailable: false,
      updatedAt: new Date(0).toISOString(),
    });
    linearHook.saveSettingsAsync.mockResolvedValue({
      enabled: false,
      hasApiToken: true,
      validationStatus: "pending",
      issueSearchAvailable: false,
      updatedAt: new Date(0).toISOString(),
    });
    linearHook.validateAsync.mockResolvedValue({
      enabled: true,
      hasApiToken: true,
      validationStatus: "valid",
      issueSearchAvailable: true,
      updatedAt: new Date(0).toISOString(),
    });
  });

  it("shows Linear issue reference configuration status", () => {
    render(<LinearIntegrationSettingsPanel />);

    expect(screen.getByText("Linear")).toBeInTheDocument();
    expect(screen.getByText("Issue references not ready")).toBeInTheDocument();
    expect(screen.queryByText("Webhook path")).not.toBeInTheDocument();
  });

  it("saves and validates the API token", async () => {
    const user = userEvent.setup();
    linearHook.state.settings.hasApiToken = true;
    render(<LinearIntegrationSettingsPanel />);

    await user.type(screen.getByLabelText("API token"), "lin_api_token");
    await user.click(screen.getByRole("button", { name: /Save API token/ }));

    expect(linearHook.saveSettingsAsync).toHaveBeenCalledWith({
      apiToken: "lin_api_token",
    });
    expect(linearHook.validateAsync).toHaveBeenCalled();
  });

  it("shows backend validation failures after saving a token", async () => {
    const user = userEvent.setup();
    linearHook.state.settings.hasApiToken = true;
    linearHook.validateAsync.mockResolvedValue({
      enabled: false,
      hasApiToken: true,
      validationStatus: "invalid",
      issueSearchAvailable: false,
      lastError: "Linear returned HTTP 401",
      updatedAt: new Date(0).toISOString(),
    });

    render(<LinearIntegrationSettingsPanel />);

    await user.type(screen.getByLabelText("API token"), "lin_api_token");
    await user.click(screen.getByRole("button", { name: /Save API token/ }));

    expect(
      await screen.findByText("Linear returned HTTP 401"),
    ).toBeInTheDocument();
  });

  it("shows string errors from the validate command", async () => {
    const user = userEvent.setup();
    linearHook.state.settings.hasApiToken = true;
    linearHook.validateAsync.mockRejectedValue(
      "Linear API token is missing from secure storage",
    );

    render(<LinearIntegrationSettingsPanel />);

    await user.click(screen.getByRole("button", { name: "Validate" }));

    expect(
      await screen.findByText(
        "Linear API token is missing from secure storage",
      ),
    ).toBeInTheDocument();
  });

  it("does not offer disconnect when nothing is configured", () => {
    render(<LinearIntegrationSettingsPanel />);

    expect(screen.getByTestId("integration-status-banner")).toHaveAttribute(
      "data-connected",
      "false",
    );
    expect(
      screen.queryByRole("button", { name: "Disconnect" }),
    ).not.toBeInTheDocument();
  });

  it("paints the status banner green when the connection is valid", () => {
    linearHook.state.settings = {
      enabled: true,
      hasApiToken: true,
      validationStatus: "valid",
      issueSearchAvailable: true,
      lastValidatedAt: new Date(0).toISOString(),
      lastError: null,
      updatedAt: new Date(0).toISOString(),
    };

    render(<LinearIntegrationSettingsPanel />);

    expect(screen.getByText("Issue references enabled")).toBeInTheDocument();
    expect(screen.getByTestId("integration-status-banner")).toHaveAttribute(
      "data-connected",
      "true",
    );
    expect(
      screen.getByRole("button", { name: "Disconnect" }),
    ).toBeInTheDocument();
  });

  it("clears the connection after confirming disconnect", async () => {
    const user = userEvent.setup();
    linearHook.state.settings.hasApiToken = true;
    render(<LinearIntegrationSettingsPanel />);

    await user.click(screen.getByRole("button", { name: "Disconnect" }));
    // First click only reveals the confirmation step; nothing cleared yet.
    expect(linearHook.disconnectAsync).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Confirm disconnect" }));

    expect(linearHook.disconnectAsync).toHaveBeenCalledTimes(1);
  });
});
