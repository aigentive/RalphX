import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createElement } from "react";

import { GranolaIntegrationSettingsPanel } from "./GranolaIntegrationSettingsPanel";

const granolaHook = vi.hoisted(() => ({
  saveSettingsAsync: vi.fn(),
  validateAsync: vi.fn(),
  disconnectAsync: vi.fn(),
  state: {
    settings: {
      enabled: false,
      hasApiToken: false,
      validationStatus: "not_configured",
      lastValidatedAt: null as string | null,
      lastError: null as string | null,
      updatedAt: new Date(0).toISOString(),
    },
    isLoading: false,
    isError: false,
    error: null as Error | null,
    connected: false,
    isSavingSettings: false,
    isValidating: false,
    isDisconnecting: false,
    saveSettingsError: null as Error | null,
    validateError: null as Error | null,
    disconnectError: null as Error | null,
  },
}));

vi.mock("@/hooks/useGranolaIntegration", () => ({
  useGranolaIntegration: () => ({
    ...granolaHook.state,
    saveSettingsAsync: granolaHook.saveSettingsAsync,
    validateAsync: granolaHook.validateAsync,
    disconnectAsync: granolaHook.disconnectAsync,
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
        createElement(GranolaIntegrationSettingsPanel),
      ),
    ),
  };
}

describe("GranolaIntegrationSettingsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    granolaHook.state.settings = {
      enabled: false,
      hasApiToken: false,
      validationStatus: "not_configured",
      lastValidatedAt: null,
      lastError: null,
      updatedAt: new Date(0).toISOString(),
    };
    granolaHook.state.isLoading = false;
    granolaHook.state.isError = false;
    granolaHook.state.error = null;
    granolaHook.state.connected = false;
    granolaHook.state.isSavingSettings = false;
    granolaHook.state.isValidating = false;
    granolaHook.state.isDisconnecting = false;
    granolaHook.state.saveSettingsError = null;
    granolaHook.state.validateError = null;
    granolaHook.state.disconnectError = null;
    granolaHook.disconnectAsync.mockResolvedValue({
      enabled: false,
      hasApiToken: false,
      validationStatus: "not_configured",
      updatedAt: new Date(0).toISOString(),
    });
    granolaHook.saveSettingsAsync.mockResolvedValue({
      enabled: false,
      hasApiToken: true,
      validationStatus: "pending",
      updatedAt: new Date(0).toISOString(),
    });
    granolaHook.validateAsync.mockResolvedValue({
      enabled: true,
      hasApiToken: true,
      validationStatus: "valid",
      updatedAt: new Date(0).toISOString(),
    });
  });

  it("shows Granola note reference configuration status", () => {
    renderPanel();

    expect(screen.getByText("Note references not ready")).toBeInTheDocument();
    expect(screen.getByText("Get a Granola API key")).toBeInTheDocument();
    expect(screen.getByText(/Settings -> Connectors -> API keys/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Granola API docs/ })).toHaveAttribute(
      "href",
      "https://docs.granola.ai/introduction",
    );
  });

  it("saves and validates the API token", async () => {
    const user = userEvent.setup();
    granolaHook.state.settings.hasApiToken = true;
    renderPanel();

    await user.type(screen.getByLabelText("API token"), "granola_token");
    await user.click(screen.getByRole("button", { name: /Save API token/ }));

    expect(granolaHook.saveSettingsAsync).toHaveBeenCalledWith({
      apiToken: "granola_token",
    });
    expect(granolaHook.validateAsync).toHaveBeenCalled();
  });

  it("rejects an empty API token before invoking the backend", async () => {
    const user = userEvent.setup();
    renderPanel();

    await user.click(screen.getByRole("button", { name: /Save API token/ }));

    expect(
      await screen.findByText("Granola API token cannot be empty"),
    ).toBeInTheDocument();
    expect(granolaHook.saveSettingsAsync).not.toHaveBeenCalled();
  });

  it("shows backend validation failures after saving a token", async () => {
    const user = userEvent.setup();
    granolaHook.state.settings.hasApiToken = true;
    granolaHook.validateAsync.mockResolvedValue({
      enabled: false,
      hasApiToken: true,
      validationStatus: "invalid",
      lastError: "Granola returned HTTP 401",
      updatedAt: new Date(0).toISOString(),
    });

    renderPanel();

    await user.type(screen.getByLabelText("API token"), "granola_token");
    await user.click(screen.getByRole("button", { name: /Save API token/ }));

    expect(
      await screen.findByText("Granola returned HTTP 401"),
    ).toBeInTheDocument();
  });

  it("shows string errors from the validate command", async () => {
    const user = userEvent.setup();
    granolaHook.state.settings.hasApiToken = true;
    granolaHook.validateAsync.mockRejectedValue(
      "Granola API token is missing from secure storage",
    );

    renderPanel();

    await user.click(screen.getByRole("button", { name: "Validate" }));

    expect(
      await screen.findByText(
        "Granola API token is missing from secure storage",
      ),
    ).toBeInTheDocument();
  });

  it("does not offer disconnect when nothing is configured", () => {
    renderPanel();

    expect(screen.getByTestId("integration-status-banner")).toHaveAttribute(
      "data-connected",
      "false",
    );
    expect(
      screen.queryByRole("button", { name: "Disconnect" }),
    ).not.toBeInTheDocument();
  });

  it("paints the status banner green when the connection is valid", () => {
    granolaHook.state.settings = {
      enabled: true,
      hasApiToken: true,
      validationStatus: "valid",
      lastValidatedAt: new Date(0).toISOString(),
      lastError: null,
      updatedAt: new Date(0).toISOString(),
    };
    granolaHook.state.connected = true;

    renderPanel();

    expect(screen.getByText("Note references enabled")).toBeInTheDocument();
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
    granolaHook.state.settings.hasApiToken = true;
    renderPanel();

    await user.click(screen.getByRole("button", { name: "Disconnect" }));
    expect(granolaHook.disconnectAsync).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Confirm disconnect" }));

    expect(granolaHook.disconnectAsync).toHaveBeenCalledTimes(1);
  });
});
