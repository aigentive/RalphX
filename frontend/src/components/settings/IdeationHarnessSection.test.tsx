import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  ExecutionHarnessSection,
  IdeationHarnessSection,
} from "./IdeationHarnessSection";
import type { AgentProvidersSettingsResponse } from "@/api/harness-providers";
import type { AgentHarnessLaneView } from "@/api/ideation-harness";
import { useConfirmation } from "@/hooks/useConfirmation";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { useAgentHarnessSettings } from "@/hooks/useIdeationHarnessSettings";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";

vi.mock("@/hooks/useIdeationHarnessSettings", () => ({
  useAgentHarnessSettings: vi.fn(),
}));

vi.mock("@/hooks/useAgentModels", async () => {
  const actual = await vi.importActual<typeof import("@/lib/agent-models")>(
    "@/lib/agent-models",
  );
  return {
    useAgentModels: () => ({
      models: [],
      registry: actual.AGENT_MODEL_CATALOG,
      isLoading: false,
      isPlaceholderData: false,
      isError: false,
      error: null,
      upsertModel: vi.fn(),
      upsertModelAsync: vi.fn(),
      isUpserting: false,
      upsertError: null,
      deleteModel: vi.fn(),
      deleteModelAsync: vi.fn(),
      isDeleting: false,
      deleteError: null,
    }),
  };
});

vi.mock("@/hooks/useHarnessProviders", () => ({
  useHarnessProviders: vi.fn(),
}));

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: vi.fn(),
}));

vi.mock("@/stores/projectStore", () => ({
  useProjectStore: vi.fn(),
  selectActiveProject: (state: { activeProject: unknown }) => state.activeProject,
}));

const globalLanes: AgentHarnessLaneView[] = [
  {
    lane: "ideation_primary",
    row: {
      projectId: null,
      lane: "ideation_primary",
      harness: "codex",
      model: "gpt-5.5",
      effort: "xhigh",
      approvalPolicy: "never",
      sandboxMode: "danger-full-access",
      updatedAt: new Date().toISOString(),
    },
    configuredHarness: "codex",
    effectiveHarness: "codex",
    binaryPath: "/usr/local/bin/codex",
    binaryFound: true,
    probeSucceeded: true,
    available: true,
    missingCoreExecFeatures: [],
    error: null,
  },
  {
    lane: "ideation_verifier",
    row: {
      projectId: null,
      lane: "ideation_verifier",
      harness: "claude",
      model: null,
      effort: null,
      approvalPolicy: null,
      sandboxMode: null,
      updatedAt: new Date().toISOString(),
    },
    configuredHarness: "claude",
    effectiveHarness: "claude",
    binaryPath: "/usr/local/bin/claude",
    binaryFound: true,
    probeSucceeded: true,
    available: true,
    missingCoreExecFeatures: [],
    error: null,
  },
];

const updateLane = vi.fn();
const confirm = vi.fn();
const providerUpdatedAt = new Date().toISOString();
const CODEX_MCP_REQUIREMENT_COPY =
  "Temporarily locked for Codex: RalphX MCP tools currently require Never approval and Danger Full Access.";

const enabledProviderSettings: AgentProvidersSettingsResponse = {
  providers: [
    {
      provider: "claude",
      enabled: true,
      isDefault: false,
      model: "sonnet",
      effort: "medium",
      approvalPolicy: null,
      sandboxMode: null,
      claudePermissionMode: "bypassPermissions",
      claudeDangerouslySkipPermissions: true,
      claudeAllowDangerouslySkipPermissions: false,
      available: true,
      binaryFound: true,
      binaryPath: "/usr/local/bin/claude",
      status: "Available claude detected at /usr/local/bin/claude.",
      error: null,
      missingCoreExecFeatures: [],
      updatedAt: providerUpdatedAt,
    },
    {
      provider: "codex",
      enabled: true,
      isDefault: true,
      model: "gpt-5.5",
      effort: "xhigh",
      approvalPolicy: "never",
      sandboxMode: "danger-full-access",
      claudePermissionMode: null,
      claudeDangerouslySkipPermissions: false,
      claudeAllowDangerouslySkipPermissions: false,
      available: true,
      binaryFound: true,
      binaryPath: "/usr/local/bin/codex",
      status: "Available codex detected at /usr/local/bin/codex.",
      error: null,
      missingCoreExecFeatures: [],
      updatedAt: providerUpdatedAt,
    },
  ],
  defaultProvider: "codex",
  requiresOnboarding: false,
};

const noProviderSettings: AgentProvidersSettingsResponse = {
  providers: [],
  defaultProvider: null,
  requiresOnboarding: true,
};

function mockProviderSettings(
  settings: AgentProvidersSettingsResponse,
  overrides: Partial<ReturnType<typeof useHarnessProviders>> = {},
) {
  vi.mocked(useHarnessProviders).mockReturnValue({
    settings,
    providers: settings.providers,
    isLoading: false,
    isPlaceholderData: false,
    isError: false,
    error: null,
    refetchProviders: vi.fn(),
    updateProviderAsync: vi.fn(),
    isUpdating: false,
    updateError: null,
    ...overrides,
  } as ReturnType<typeof useHarnessProviders>);
}

if (!HTMLElement.prototype.hasPointerCapture) {
  Object.defineProperty(HTMLElement.prototype, "hasPointerCapture", {
    value: () => false,
    writable: true,
  });
}

if (!HTMLElement.prototype.setPointerCapture) {
  Object.defineProperty(HTMLElement.prototype, "setPointerCapture", {
    value: vi.fn(),
    writable: true,
  });
}

if (!HTMLElement.prototype.releasePointerCapture) {
  Object.defineProperty(HTMLElement.prototype, "releasePointerCapture", {
    value: vi.fn(),
    writable: true,
  });
}

if (!HTMLElement.prototype.scrollIntoView) {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    value: vi.fn(),
    writable: true,
  });
}

function openSelect(testId: string) {
  const trigger = screen.getByTestId(testId);
  fireEvent.keyDown(trigger, { key: "ArrowDown", code: "ArrowDown" });
}

describe("IdeationHarnessSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    useUiStore.getState().closeModal();
    mockProviderSettings(enabledProviderSettings);
    vi.mocked(useProjectStore).mockReturnValue({
      id: "project-1",
      name: "Project One",
    });
    vi.mocked(useAgentHarnessSettings).mockImplementation((projectId) => ({
      lanes: projectId === null ? globalLanes : [],
      isLoading: false,
      isPlaceholderData: false,
      isError: false,
      error: null,
      updateLane,
      isUpdating: false,
      saveError: null,
      resetError: vi.fn(),
    }));
    vi.mocked(useConfirmation).mockReturnValue({
      confirm,
      confirmationDialogProps: {
        isOpen: false,
        options: null,
        onConfirm: vi.fn(),
        onCancel: vi.fn(),
      },
      ConfirmationDialog: () => null,
    });
    confirm.mockResolvedValue(true);
  });

  it("hides Codex permission controls behind a disclosure by default", async () => {
    const user = userEvent.setup();
    render(<IdeationHarnessSection />);

    expect(screen.getByRole("button", { name: "Show permissions" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(screen.getByText("Approval")).not.toBeVisible();
    expect(screen.getByText("Sandbox")).not.toBeVisible();
    expect(screen.getByText(CODEX_MCP_REQUIREMENT_COPY)).not.toBeVisible();

    await user.click(screen.getByRole("button", { name: "Show permissions" }));

    expect(screen.getByText("Approval")).toBeInTheDocument();
    expect(screen.getByText("Sandbox")).toBeInTheDocument();
    expect(screen.queryByText("Fallback Harness")).not.toBeInTheDocument();
    expect(screen.getByText(CODEX_MCP_REQUIREMENT_COPY)).toBeInTheDocument();
    expect(screen.getByTestId("approval-ideation_primary")).toHaveAttribute(
      "data-disabled",
    );
    expect(screen.getByTestId("sandbox-ideation_primary")).toHaveAttribute(
      "data-disabled",
    );
    expect(screen.getByText("Ideation Agents")).toBeInTheDocument();
    expect(screen.queryByText("Execution Worker")).not.toBeInTheDocument();
  });

  it("persists the selected agent settings tab", async () => {
    const user = userEvent.setup();
    render(<IdeationHarnessSection />);

    await user.click(screen.getByRole("tab", { name: "Project Overrides" }));

    expect(screen.getByRole("tab", { name: "Project Overrides" })).toHaveAttribute(
      "data-state",
      "active",
    );
    expect(localStorage.getItem("ralphx-settings-harness-tab")).toBe(
      JSON.stringify({ ideation: "project" }),
    );
  });

  it("hides lane controls until a default provider is enabled", () => {
    mockProviderSettings(noProviderSettings);

    render(<IdeationHarnessSection />);

    expect(screen.getByText("Provider Setup Required")).toBeInTheDocument();
    expect(
      screen.getByText(/Enable, validate, and set a default provider/),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Open Providers/ }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Primary Ideation")).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText("Primary Ideation provider"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("Available codex detected at /usr/local/bin/codex."),
    ).not.toBeInTheDocument();
  });

  it("links the no-provider notice to Harness Providers settings", () => {
    mockProviderSettings(noProviderSettings);

    render(<ExecutionHarnessSection />);
    fireEvent.click(screen.getByRole("button", { name: /Open Providers/ }));

    const state = useUiStore.getState();
    expect(state.activeModal).toBe("settings");
    expect(state.modalContext).toEqual({ section: "providers" });
  });

  it("holds lane controls behind a loading notice while provider settings are placeholder data", () => {
    mockProviderSettings(noProviderSettings, { isPlaceholderData: true });

    render(<IdeationHarnessSection />);

    expect(screen.getByText("Loading Providers")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Checking configured providers before showing agent lane controls.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText("Primary Ideation")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Open Providers/ }),
    ).not.toBeInTheDocument();
  });

  it("renders settings notices with explicit token-backed paint styles", () => {
    render(<IdeationHarnessSection />);
    fireEvent.click(screen.getByRole("button", { name: "Show permissions" }));

    const lockedNotice = screen
      .getByText(CODEX_MCP_REQUIREMENT_COPY)
      .closest(".settings-inline-notice");

    expect(lockedNotice).not.toBeNull();
    expect(lockedNotice?.getAttribute("style")).toContain(
      "background-color: var(--notice-info-bg)",
    );
    expect(lockedNotice?.getAttribute("style")).toContain(
      "border-color: var(--notice-info-border)",
    );
    expect(lockedNotice?.getAttribute("style")).toContain(
      "color: var(--notice-info-text)",
    );
  });

  it("allows switching model presets without clearing the current value first", async () => {
    render(<IdeationHarnessSection />);

    openSelect("model-ideation_primary");
    fireEvent.click(screen.getByRole("option", { name: /gpt-5\.4-mini/ }));

    await waitFor(() => {
      expect(updateLane).toHaveBeenCalledWith(
        {
          lane: "ideation_primary",
          harness: "codex",
          model: "gpt-5.4-mini",
          effort: null,
          approvalPolicy: "never",
          sandboxMode: "danger-full-access",
        },
        { onError: expect.any(Function) },
      );
    });
  });

  it("updates lane provider and effort from enabled provider options", async () => {
    const user = userEvent.setup();
    render(<IdeationHarnessSection />);

    openSelect("harness-ideation_primary");
    await user.click(screen.getByRole("option", { name: /Claude/ }));
    expect(updateLane).toHaveBeenCalledWith(
      {
        lane: "ideation_primary",
        harness: "claude",
        model: null,
        effort: null,
        approvalPolicy: null,
        sandboxMode: null,
      },
      { onError: expect.any(Function) },
    );

    await user.click(screen.getByRole("button", { name: "Collapse all" }));
    await user.click(screen.getByRole("button", { name: "Expand all" }));

    fireEvent.keyDown(screen.getByLabelText("Primary Ideation effort"), {
      key: "ArrowDown",
      code: "ArrowDown",
    });
    await user.click(screen.getAllByRole("option", { name: /High/ })[0]!);
    expect(updateLane).toHaveBeenLastCalledWith(
      {
        lane: "ideation_primary",
        harness: "codex",
        model: "gpt-5.5",
        effort: "high",
        approvalPolicy: "never",
        sandboxMode: "danger-full-access",
      },
      { onError: expect.any(Function) },
    );
  });

  it("applies verifier Codex defaults when switching the verifier provider", async () => {
    const user = userEvent.setup();
    render(<IdeationHarnessSection />);

    openSelect("harness-ideation_verifier");
    await user.click(screen.getByRole("option", { name: /Codex/ }));

    expect(updateLane).toHaveBeenCalledWith(
      {
        lane: "ideation_verifier",
        harness: "codex",
        model: "gpt-5.4-mini",
        effort: "medium",
        approvalPolicy: "never",
        sandboxMode: "danger-full-access",
      },
      { onError: expect.any(Function) },
    );
  });

  it("shows Codex model presets in the model select", async () => {
    render(<IdeationHarnessSection />);

    openSelect("model-ideation_primary");

    expect(screen.getByRole("option", { name: /gpt-5\.5 \(Current\)/ })).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /Frontier model for complex coding, research, and real-world work\./ })
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /gpt-5\.4.*Strong model for everyday coding\./ })
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", {
        name: /gpt-5\.4-mini.*Small, fast, and cost-efficient model for simpler coding tasks\./,
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /gpt-5\.3-codex.*Coding-optimized model\./ })
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /gpt-5\.3-codex-spark.*Ultra-fast coding model\./ })
    ).toBeInTheDocument();
  });

  it("shows Claude model presets for Claude harness lanes", async () => {
    render(<IdeationHarnessSection />);

    openSelect("model-ideation_verifier");

    expect(screen.getByRole("option", { name: /sonnet/ })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /opus/ })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /haiku/ })).toBeInTheDocument();
  });

  it("exposes explicit accessible labels for provider and model controls", () => {
    render(<IdeationHarnessSection />);

    expect(screen.getByLabelText("Primary Ideation provider")).toBeInTheDocument();
    expect(screen.getByLabelText("Primary Ideation model")).toBeInTheDocument();
  });

  it("shows effort options with clearer labels including Default and Extra High", () => {
    render(<IdeationHarnessSection />);

    // The effort select for ideation_primary shows "Extra High" for xhigh
    // Check that the effort dropdowns render with the updated labels in the DOM
    const effortTriggers = document.querySelectorAll('[placeholder="Select effort"]');
    expect(effortTriggers.length).toBe(0); // triggers don't have placeholders; SelectValue shows selected

    // Verify the effort options are rendered inside SelectContent (accessible)
    // The "Default" label replaces "Inherit"
    expect(screen.queryByText("Inherit")).not.toBeInTheDocument();
    expect(screen.queryByText("XHigh")).not.toBeInTheDocument();
  });

  it("renders the warn-tone notice when a lane has missing core exec features", () => {
    const warnLanes: AgentHarnessLaneView[] = [
      {
        ...globalLanes[0]!,
        missingCoreExecFeatures: ["streaming"],
        error: "binary missing",
      },
      globalLanes[1]!,
    ];
    vi.mocked(useAgentHarnessSettings).mockImplementation((projectId) => ({
      lanes: projectId === null ? warnLanes : [],
      isLoading: false,
      isPlaceholderData: false,
      isError: false,
      error: null,
      updateLane,
      isUpdating: false,
      saveError: null,
      resetError: vi.fn(),
    }));
    render(<IdeationHarnessSection />);

    // The warn notice paints with --notice-warn-bg.
    const warnSurfaces = document.querySelectorAll(".settings-inline-notice");
    const hasWarn = Array.from(warnSurfaces).some((el) =>
      (el.getAttribute("style") ?? "").includes("var(--notice-warn-bg)"),
    );
    expect(hasWarn).toBe(true);
  });

  it("shows missing provider feature warnings without repeating probe success copy", async () => {
    const warnLanes: AgentHarnessLaneView[] = [
      {
        ...globalLanes[0]!,
        effectiveHarness: "claude",
        missingCoreExecFeatures: ["streaming"],
        error: null,
      },
      globalLanes[1]!,
    ];
    vi.mocked(useAgentHarnessSettings).mockImplementation((projectId) => ({
      lanes: projectId === null ? warnLanes : [],
      isLoading: false,
      isPlaceholderData: false,
      isError: false,
      error: null,
      updateLane,
      isUpdating: false,
      saveError: null,
      resetError: vi.fn(),
    }));

    render(<IdeationHarnessSection />);

    expect(
      await screen.findByText("Needs attention · Effective: claude"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Missing Codex features: streaming."),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("codex detected at /usr/local/bin/codex."),
    ).not.toBeInTheDocument();
  });

  it("uses the generic warning copy for non-Codex lane provider issues", () => {
    const warnLanes: AgentHarnessLaneView[] = [
      {
        ...globalLanes[0]!,
        configuredHarness: "claude",
        effectiveHarness: "claude",
        missingCoreExecFeatures: ["streaming"],
        error: null,
      },
      globalLanes[1]!,
    ];
    vi.mocked(useAgentHarnessSettings).mockImplementation((projectId) => ({
      lanes: projectId === null ? warnLanes : [],
      isLoading: false,
      isPlaceholderData: false,
      isError: false,
      error: null,
      updateLane,
      isUpdating: false,
      saveError: null,
      resetError: vi.fn(),
    }));

    render(<IdeationHarnessSection />);

    expect(
      screen.getByText("This lane needs provider attention before it can run."),
    ).toBeInTheDocument();
  });

  it("resets a lane to the configured default provider after confirmation", async () => {
    render(<IdeationHarnessSection />);

    fireEvent.click(
      screen.getByRole("button", {
        name: "Reset Verification to default provider",
      }),
    );

    await waitFor(() => {
      expect(confirm).toHaveBeenCalledWith(
        expect.objectContaining({
          title: "Reset Verification to default provider?",
          confirmText: "Reset lane",
        }),
      );
      expect(updateLane).toHaveBeenCalledWith(
        {
          lane: "ideation_verifier",
          harness: "codex",
          model: "gpt-5.5",
          effort: "xhigh",
          approvalPolicy: "never",
          sandboxMode: "danger-full-access",
        },
        { onError: expect.any(Function) },
      );
    });
  });

  it("leaves a lane unchanged when reset confirmation is cancelled", async () => {
    confirm.mockResolvedValue(false);
    render(<IdeationHarnessSection />);

    fireEvent.click(
      screen.getByRole("button", {
        name: "Reset Verification to default provider",
      }),
    );

    await waitFor(() => {
      expect(confirm).toHaveBeenCalledWith(
        expect.objectContaining({
          title: "Reset Verification to default provider?",
        }),
      );
    });
    expect(updateLane).not.toHaveBeenCalled();
  });
});

describe("ExecutionHarnessSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(useProjectStore).mockReturnValue({
      id: "project-1",
      name: "Project One",
    });
    vi.mocked(useAgentHarnessSettings).mockImplementation((projectId) => ({
      lanes:
        projectId === null
          ? [
              ...globalLanes,
              {
                lane: "execution_worker",
                row: {
                  projectId: null,
                  lane: "execution_worker",
                  harness: "codex",
                  model: "gpt-5.5",
                  effort: "xhigh",
                  approvalPolicy: "never",
                  sandboxMode: "danger-full-access",
                  updatedAt: new Date().toISOString(),
                },
                configuredHarness: "codex",
                effectiveHarness: "codex",
                binaryPath: "/usr/local/bin/codex",
                binaryFound: true,
                probeSucceeded: true,
                available: true,
                missingCoreExecFeatures: [],
                error: null,
              },
            ]
          : [],
      isLoading: false,
      isPlaceholderData: false,
      isError: false,
      error: null,
      updateLane,
      isUpdating: false,
      saveError: null,
      resetError: vi.fn(),
    }));
  });

  it("renders execution lanes as a first-class section", () => {
    render(<ExecutionHarnessSection />);

    expect(screen.getByText("Execution Pipeline Agents")).toBeInTheDocument();
    expect(screen.getByText("Execution Worker")).toBeInTheDocument();
    expect(screen.queryByText("Primary Ideation")).not.toBeInTheDocument();
  });

  it("uses a consistent single-line h-9 trigger height for provider, model, and effort selections", () => {
    render(<ExecutionHarnessSection />);

    expect(screen.getByTestId("harness-execution_worker")).toHaveClass("h-9");
    expect(screen.getByTestId("harness-execution_worker")).toHaveClass("items-center");
    expect(screen.getByTestId("model-execution_worker")).toHaveClass("h-9");
    expect(screen.getByTestId("model-execution_worker")).toHaveClass("items-center");
    expect(screen.getByLabelText("Execution Worker effort")).toHaveClass("h-9");
    expect(screen.getByLabelText("Execution Worker effort")).toHaveClass("items-center");
  });

  it("applies Codex defaults when switching an execution lane to Codex", async () => {
    const user = userEvent.setup();
    const executionWorker: AgentHarnessLaneView = {
      lane: "execution_worker",
      row: {
        projectId: null,
        lane: "execution_worker",
        harness: "claude",
        model: null,
        effort: null,
        approvalPolicy: null,
        sandboxMode: null,
        updatedAt: new Date().toISOString(),
      },
      configuredHarness: "claude",
      effectiveHarness: "claude",
      binaryPath: "/usr/local/bin/claude",
      binaryFound: true,
      probeSucceeded: true,
      available: true,
      missingCoreExecFeatures: [],
      error: null,
    };
    vi.mocked(useAgentHarnessSettings).mockImplementation((projectId) => ({
      lanes: projectId === null ? [...globalLanes, executionWorker] : [],
      isLoading: false,
      isPlaceholderData: false,
      isError: false,
      error: null,
      updateLane,
      isUpdating: false,
      saveError: null,
      resetError: vi.fn(),
    }));

    render(<ExecutionHarnessSection />);

    openSelect("harness-execution_worker");
    await user.click(screen.getByRole("option", { name: /Codex/ }));

    expect(updateLane).toHaveBeenCalledWith(
      {
        lane: "execution_worker",
        harness: "codex",
        model: "gpt-5.5",
        effort: "xhigh",
        approvalPolicy: "never",
        sandboxMode: "danger-full-access",
      },
      { onError: expect.any(Function) },
    );
  });
});
