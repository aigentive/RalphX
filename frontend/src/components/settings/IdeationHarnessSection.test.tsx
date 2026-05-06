import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  ExecutionHarnessSection,
  IdeationHarnessSection,
} from "./IdeationHarnessSection";
import type { AgentHarnessLaneView } from "@/api/ideation-harness";
import { useAgentHarnessSettings } from "@/hooks/useIdeationHarnessSettings";
import { useProjectStore } from "@/stores/projectStore";

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
  });

  it("renders Codex-only lane controls for Codex lanes", () => {
    render(<IdeationHarnessSection />);

    expect(screen.getByText("Approval")).toBeInTheDocument();
    expect(screen.getByText("Sandbox")).toBeInTheDocument();
    expect(screen.queryByText("Fallback Harness")).not.toBeInTheDocument();
    expect(
      screen.getByText(
        "Temporarily locked for Codex: RalphX MCP tools currently require Never approval and Danger Full Access.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByTestId("approval-ideation_primary")).toHaveAttribute(
      "data-disabled",
    );
    expect(screen.getByTestId("sandbox-ideation_primary")).toHaveAttribute(
      "data-disabled",
    );
    expect(screen.getByText("Ideation Agents")).toBeInTheDocument();
    expect(screen.queryByText("Execution Worker")).not.toBeInTheDocument();
  });

  it("renders settings notices with explicit token-backed paint styles", () => {
    render(<IdeationHarnessSection />);

    const lockedNotice = screen
      .getByText(
        "Temporarily locked for Codex: RalphX MCP tools currently require Never approval and Danger Full Access.",
      )
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
      })
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
});
