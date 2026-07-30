import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { IntegrationsHubSection } from "./IntegrationsHubSection";

const hooks = vi.hoisted(() => ({
  atlassian: { settings: undefined as unknown, isLoading: false },
  github: { data: undefined as unknown, isLoading: false },
  linear: { settings: undefined as unknown, isLoading: false },
  clickup: { connected: false, isLoading: false },
  granola: { connected: false, isLoading: false },
  apiKeys: { data: undefined as unknown, isLoading: false },
  // The hub reads the flag to decide whether the remote-access/connections cards exist.
  // Mocked like every other hook here so the component needs no QueryClientProvider.
  featureFlags: { data: undefined as unknown },
}));

vi.mock("@/hooks/useAtlassianIntegration", async () => {
  const actual = await vi.importActual<
    typeof import("@/hooks/useAtlassianIntegration")
  >("@/hooks/useAtlassianIntegration");
  return { ...actual, useAtlassianIntegration: () => hooks.atlassian };
});
vi.mock("@/hooks/useGitHubConnectionStatus", () => ({
  useGitHubConnectionStatus: () => hooks.github,
}));
vi.mock("@/hooks/useLinearIntegration", async () => {
  const actual = await vi.importActual<
    typeof import("@/hooks/useLinearIntegration")
  >("@/hooks/useLinearIntegration");
  return { ...actual, useLinearIntegration: () => hooks.linear };
});
vi.mock("@/hooks/useClickUpIntegration", () => ({
  useClickUpIntegration: () => hooks.clickup,
}));
vi.mock("@/hooks/useGranolaIntegration", () => ({
  useGranolaIntegration: () => hooks.granola,
}));
vi.mock("@/hooks/useApiKeys", () => ({
  useApiKeys: () => hooks.apiKeys,
}));
vi.mock("@/hooks/useFeatureFlags", async () => {
  const actual =
    await vi.importActual<typeof import("@/hooks/useFeatureFlags")>(
      "@/hooks/useFeatureFlags",
    );
  return { ...actual, useFeatureFlags: () => hooks.featureFlags };
});

function renderHub() {
  const onNavigate = vi.fn();
  const onWarmSection = vi.fn();
  render(
    <IntegrationsHubSection
      onNavigate={onNavigate}
      onWarmSection={onWarmSection}
    />,
  );
  return { onNavigate, onWarmSection };
}

function card(name: string): HTMLElement {
  return screen.getByTestId(`integration-card-${name}`);
}

describe("IntegrationsHubSection", () => {
  beforeEach(() => {
    hooks.atlassian = { settings: undefined, isLoading: false };
    hooks.github = { data: undefined, isLoading: false };
    hooks.linear = { settings: undefined, isLoading: false };
    hooks.clickup = { connected: false, isLoading: false };
    hooks.granola = { connected: false, isLoading: false };
    hooks.apiKeys = { data: undefined, isLoading: false };
    hooks.featureFlags = { data: undefined };
  });

  it("renders a card for every integration and external-access target", () => {
    renderHub();
    for (const id of [
      "integrations",
      "github",
      "linear",
      "clickup",
      "granola",
      "api-keys",
      "external-mcp",
    ]) {
      expect(card(id)).toBeInTheDocument();
    }
  });

  it("shows the set-up affordance when nothing is connected", () => {
    renderHub();
    expect(card("github")).toHaveAttribute("data-connected", "false");
    expect(
      within(card("github")).getByRole("button", { name: /set up github/i }),
    ).toBeInTheDocument();
  });

  it("shows the manage affordance for connected providers", () => {
    hooks.github = { data: { state: "authenticated" }, isLoading: false };
    hooks.clickup = { connected: true, isLoading: false };
    hooks.granola = { connected: true, isLoading: false };
    hooks.atlassian = { settings: { enabled: true }, isLoading: false };
    hooks.linear = {
      settings: {
        enabled: true,
        hasApiToken: true,
        validationStatus: "valid",
        issueSearchAvailable: true,
      },
      isLoading: false,
    };
    renderHub();
    for (const id of ["integrations", "github", "linear", "clickup", "granola"]) {
      expect(card(id)).toHaveAttribute("data-connected", "true");
      expect(
        within(card(id)).getByRole("button", { name: /^manage /i }),
      ).toBeInTheDocument();
    }
  });

  it("navigates to the drill-in leaf through the supplied section setter", async () => {
    const user = userEvent.setup();
    const { onNavigate } = renderHub();
    await user.click(
      within(card("linear")).getByRole("button", { name: /set up linear/i }),
    );
    expect(onNavigate).toHaveBeenCalledWith("linear");
  });

  it("warms the drill-in module on card hover", async () => {
    const user = userEvent.setup();
    const { onWarmSection } = renderHub();
    await user.hover(card("granola"));
    expect(onWarmSection).toHaveBeenCalledWith("granola");
  });

  it("degrades a failed status read to the not-connected state", () => {
    hooks.github = { data: undefined, isLoading: false };
    renderHub();
    expect(card("github")).toHaveAttribute("data-connected", "false");
    expect(
      within(card("github")).getByRole("button", { name: /set up github/i }),
    ).toBeEnabled();
  });

  it("summarises stored API keys once the list resolves", () => {
    hooks.apiKeys = { data: [{ id: "a" }, { id: "b" }], isLoading: false };
    renderHub();
    expect(within(card("api-keys")).getByText(/2 keys/i)).toBeInTheDocument();
  });
});
