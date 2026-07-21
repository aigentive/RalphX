/**
 * IdeationSettingsPanel Tests
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  IdeationSettingsContent,
  IdeationSettingsPanel,
} from "./IdeationSettingsPanel";
import { ideationApi } from "@/api/ideation";
import { useIdeationSettings } from "@/hooks/useIdeationSettings";
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

function SurfaceHarness({ surface }: { surface: "tasks" | "planning" }) {
  const controller = useIdeationSettings();
  return <IdeationSettingsContent controller={controller} surface={surface} />;
}

describe("IdeationSettingsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(ideationApi.settings.get).mockResolvedValue(defaultSettings);
  });

  it("renders the compatibility wrapper as Tasks", () => {
    render(<IdeationSettingsPanel />, { wrapper: createWrapper() });

    expect(screen.getByText("Tasks")).toBeInTheDocument();
    expect(screen.getByText("Configure task and acceptance gates")).toBeInTheDocument();
  });

  it("keeps automatic verification out of the Tasks surface", async () => {
    const user = userEvent.setup();
    render(<SurfaceHarness surface="tasks" />, { wrapper: createWrapper() });

    expect(await screen.findByTestId("enable-tasks")).toBeInTheDocument();
    expect(screen.queryByTestId("auto-verify-plans")).not.toBeInTheDocument();

    await user.click(screen.getByTestId("external-overrides-toggle"));
    expect(screen.getByTestId("ext-override-verification-for-accept")).toBeInTheDocument();
    expect(screen.queryByTestId("ext-override-auto-verify-plans")).not.toBeInTheDocument();
  });

  it("isolates automatic verification on the Planning surface", async () => {
    const user = userEvent.setup();
    render(<SurfaceHarness surface="planning" />, { wrapper: createWrapper() });

    expect(await screen.findByTestId("auto-verify-plans")).toBeInTheDocument();
    expect(screen.queryByTestId("enable-tasks")).not.toBeInTheDocument();
    expect(screen.queryByTestId("require-verification-for-accept")).not.toBeInTheDocument();

    await user.click(screen.getByTestId("external-overrides-toggle"));
    expect(screen.getByTestId("ext-override-auto-verify-plans")).toBeInTheDocument();
    expect(screen.queryByTestId("ext-override-verification-for-accept")).not.toBeInTheDocument();
  });

  it("renders automatic and acceptance verification controls", async () => {
    render(<IdeationSettingsPanel />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByTestId("require-accept-for-finalize")).toBeInTheDocument();
      expect(screen.getByTestId("require-verification-for-accept")).toBeInTheDocument();
      expect(screen.getByTestId("auto-verify-plans")).toBeInTheDocument();
      expect(screen.getByText("Verify automatically on acceptance")).toBeInTheDocument();
      expect(
        screen.getByText(
          "When verification is required, an acceptance attempt queues a visible Verify Plan turn instead of interrupting drafting",
        ),
      ).toBeInTheDocument();
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
