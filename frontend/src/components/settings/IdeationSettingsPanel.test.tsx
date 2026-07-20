/**
 * IdeationSettingsPanel Tests
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { IdeationSettingsPanel } from "./IdeationSettingsPanel";
import { ideationApi } from "@/api/ideation";
import type { IdeationSettings } from "@/types/ideation-config";

// Mock the ideation API
vi.mock("@/api/ideation", () => ({
  ideationApi: {
    settings: {
      get: vi.fn(),
      update: vi.fn(),
    },
  },
}));

// Mock uiStore for autoAcceptPlans
vi.mock("@/stores/uiStore", () => ({
  useUiStore: (selector: (s: { autoAcceptPlans: boolean; setAutoAcceptPlans: () => void }) => unknown) =>
    selector({ autoAcceptPlans: false, setAutoAcceptPlans: vi.fn() }),
}));

const defaultSettings: IdeationSettings = {
  tasksEnabled: false,
  autoVerifyDraftPlans: true,
  autoVerifyPlans: false,
  requireAcceptForFinalize: false,
  requireVerificationForAccept: false,
  externalOverrides: {
    autoVerifyPlans: null,
    requireVerificationForAccept: null,
    requireAcceptForFinalize: null,
  },
};

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

describe("IdeationSettingsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(ideationApi.settings.get).mockResolvedValue(defaultSettings);
  });

  it("renders section with ShieldCheck icon and Planning & Verification title", async () => {
    render(<IdeationSettingsPanel />, { wrapper: createWrapper() });

    expect(screen.getByText("Planning & Verification")).toBeInTheDocument();
    expect(screen.getByText("Configure acceptance and verification gates")).toBeInTheDocument();
  });

  it("renders independent completion and acceptance verification controls", async () => {
    render(<IdeationSettingsPanel />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByTestId("require-accept-for-finalize")).toBeInTheDocument();
      expect(screen.getByTestId("require-verification-for-accept")).toBeInTheDocument();
      expect(screen.getByTestId("auto-verify-plans")).toBeInTheDocument();
      expect(screen.getByTestId("auto-verify-draft-plans")).toBeChecked();
      expect(screen.getByText("Verify draft plans automatically")).toBeInTheDocument();
      expect(screen.getByText("Queue missing verification on acceptance")).toBeInTheDocument();
      expect(
        screen.getByText(
          "After a successful Plan-mode Agent response, queue a visible Verify Plan turn in the same conversation",
        ),
      ).toBeInTheDocument();
    });
  });

  it("persists completion-triggered verification without changing the acceptance fallback", async () => {
    const user = userEvent.setup();
    vi.mocked(ideationApi.settings.update).mockResolvedValue({
      ...defaultSettings,
      autoVerifyDraftPlans: false,
    });
    render(<IdeationSettingsPanel />, { wrapper: createWrapper() });

    await user.click(await screen.findByTestId("auto-verify-draft-plans"));

    await waitFor(() => {
      expect(ideationApi.settings.update).toHaveBeenCalledWith(
        expect.objectContaining({
          autoVerifyDraftPlans: false,
          autoVerifyPlans: false,
        }),
      );
    });
  });

  it("renders Tasks disabled by default and persists an enable request", async () => {
    const user = userEvent.setup();
    vi.mocked(ideationApi.settings.update).mockResolvedValue({
      ...defaultSettings,
      tasksEnabled: true,
    });
    render(<IdeationSettingsPanel />, { wrapper: createWrapper() });

    const checkbox = await screen.findByTestId("enable-tasks");
    expect(checkbox).not.toBeChecked();
    await user.click(checkbox);

    await waitFor(() => {
      expect(ideationApi.settings.update).toHaveBeenCalledWith(
        expect.objectContaining({ tasksEnabled: true }),
      );
    });
  });

  it("renders the auto-accept finalization toggle", async () => {
    render(<IdeationSettingsPanel />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByTestId("auto-accept-plans")).toBeInTheDocument();
      expect(screen.getByText("Skip finalization confirmation")).toBeInTheDocument();
    });
  });

  it("calls update when require-accept-for-finalize is toggled", async () => {
    const user = userEvent.setup();
    vi.mocked(ideationApi.settings.update).mockResolvedValue({
      ...defaultSettings,
      requireAcceptForFinalize: true,
    });

    render(<IdeationSettingsPanel />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByTestId("require-accept-for-finalize")).toBeInTheDocument();
    });

    const checkbox = screen.getByTestId("require-accept-for-finalize");
    await user.click(checkbox);

    await waitFor(() => {
      expect(ideationApi.settings.update).toHaveBeenCalledWith(
        expect.objectContaining({
          requireAcceptForFinalize: true,
        })
      );
    });
  });

  it("does not render stale planMode, requirePlanApproval, suggestPlansForComplex, or autoLinkProposals controls", async () => {
    render(<IdeationSettingsPanel />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByTestId("require-accept-for-finalize")).toBeInTheDocument();
    });

    expect(screen.queryByTestId("plan-mode-required")).not.toBeInTheDocument();
    expect(screen.queryByTestId("plan-mode-optional")).not.toBeInTheDocument();
    expect(screen.queryByTestId("require-plan-approval")).not.toBeInTheDocument();
    expect(screen.queryByTestId("suggest-plans-for-complex")).not.toBeInTheDocument();
    expect(screen.queryByTestId("auto-link-proposals")).not.toBeInTheDocument();
  });

  it("renders external overrides toggle button", async () => {
    render(<IdeationSettingsPanel />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByTestId("external-overrides-toggle")).toBeInTheDocument();
      expect(screen.getByText("External Session Overrides")).toBeInTheDocument();
    });
  });

  it("shows external override selects when section is expanded", async () => {
    const user = userEvent.setup();
    render(<IdeationSettingsPanel />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByTestId("external-overrides-toggle")).toBeInTheDocument();
    });

    // Overrides not visible initially
    expect(screen.queryByTestId("ext-override-verification-for-accept")).not.toBeInTheDocument();

    // Click to expand
    await user.click(screen.getByTestId("external-overrides-toggle"));

    await waitFor(() => {
      expect(screen.getByTestId("ext-override-verification-for-accept")).toBeInTheDocument();
      expect(screen.getByTestId("ext-override-auto-verify-plans")).toBeInTheDocument();
      expect(screen.getByTestId("ext-override-accept-for-finalize")).toBeInTheDocument();
    });
  });

  it("renders external override selects with inherit as default value", async () => {
    const user = userEvent.setup();
    render(<IdeationSettingsPanel />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByTestId("external-overrides-toggle")).toBeInTheDocument();
    });

    // Expand external overrides
    await user.click(screen.getByTestId("external-overrides-toggle"));

    await waitFor(() => {
      // Each select trigger should show "Inherit" since all overrides are null
      const triggers = screen.getAllByRole("combobox");
      const overrideTriggers = triggers.filter((t) =>
        t.getAttribute("data-testid")?.startsWith("ext-override-")
      );
      expect(overrideTriggers).toHaveLength(3);
      overrideTriggers.forEach((trigger) => {
        expect(trigger).toHaveTextContent("Inherit");
      });
    });
  });
});
