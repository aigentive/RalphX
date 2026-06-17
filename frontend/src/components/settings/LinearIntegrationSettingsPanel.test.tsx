import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { LinearIntegrationSettingsPanel } from "./LinearIntegrationSettingsPanel";

const linearHook = vi.hoisted(() => ({
  saveWebhookSigningSecretAsync: vi.fn(),
  state: {
    webhookConfig: {
      enabled: false,
      hasSigningSecret: false,
    },
    isLoading: false,
    isError: false,
    error: null as Error | null,
    isSavingWebhookSigningSecret: false,
    saveWebhookSigningSecretError: null as Error | null,
  },
}));

vi.mock("@/hooks/useLinearIntegration", () => ({
  useLinearIntegration: () => ({
    ...linearHook.state,
    saveWebhookSigningSecretAsync: linearHook.saveWebhookSigningSecretAsync,
  }),
}));

describe("LinearIntegrationSettingsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    linearHook.state.webhookConfig = {
      enabled: false,
      hasSigningSecret: false,
    };
    linearHook.state.isLoading = false;
    linearHook.state.isError = false;
    linearHook.state.error = null;
    linearHook.state.isSavingWebhookSigningSecret = false;
    linearHook.state.saveWebhookSigningSecretError = null;
    linearHook.saveWebhookSigningSecretAsync.mockResolvedValue({
      enabled: true,
      hasSigningSecret: true,
    });
  });

  it("shows Linear webhook configuration status", () => {
    render(<LinearIntegrationSettingsPanel />);

    expect(screen.getByText("Linear")).toBeInTheDocument();
    expect(screen.getByText("Not configured")).toBeInTheDocument();
    expect(screen.getByDisplayValue("/api/integrations/linear/webhook")).toBeInTheDocument();
  });

  it("saves the signing secret and enables Linear webhooks", async () => {
    const user = userEvent.setup();
    render(<LinearIntegrationSettingsPanel />);

    await user.type(screen.getByLabelText("Signing secret"), "lin_secret");
    await user.click(screen.getByRole("button", { name: "Save and enable" }));

    expect(linearHook.saveWebhookSigningSecretAsync).toHaveBeenCalledWith({
      signingSecret: "lin_secret",
      enabled: true,
    });
    expect(await screen.findByText("Saved")).toBeInTheDocument();
  });
});
