import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentProvidersSettingsResponse } from "@/api/harness-providers";
import type { WorkspaceReviewRuntimeSettingsResponse } from "@/api/workspace-review-settings";
import { AGENT_MODEL_CATALOG } from "@/lib/agent-models";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import {
  useReviewSettings,
  useUpdateReviewSettings,
} from "@/hooks/useReviewSettings";
import { useWorkspaceReviewRuntimeSettings } from "@/hooks/useWorkspaceReviewSettings";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";

import WorkspaceReviewSection from "./WorkspaceReviewSection";

vi.mock("@/hooks/useAgentModels", () => ({
  useAgentModels: vi.fn(),
}));

vi.mock("@/hooks/useHarnessProviders", () => ({
  useHarnessProviders: vi.fn(),
}));

vi.mock("@/hooks/useReviewSettings", () => ({
  useReviewSettings: vi.fn(),
  useUpdateReviewSettings: vi.fn(),
}));

vi.mock("@/hooks/useWorkspaceReviewSettings", () => ({
  useWorkspaceReviewRuntimeSettings: vi.fn(),
}));

vi.mock("@/stores/projectStore", () => ({
  useProjectStore: vi.fn(),
  selectActiveProject: (state: { activeProject: unknown }) =>
    state.activeProject,
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: vi.fn(),
}));

const providerUpdatedAt = new Date().toISOString();
const updateRuntimeSettings = vi.fn();
const updateReviewSettings = vi.fn();
const openModal = vi.fn();

const enabledProviderSettings: AgentProvidersSettingsResponse = {
  providers: [
    {
      provider: "claude",
      enabled: true,
      isDefault: false,
      model: "sonnet",
      effort: "medium",
      serviceTier: null,
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
      supportsFastMode: false,
      fastModeSupportedModels: [],
      updatedAt: providerUpdatedAt,
    },
    {
      provider: "codex",
      enabled: true,
      isDefault: true,
      model: "gpt-5.5",
      effort: "xhigh",
      serviceTier: null,
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
      supportsFastMode: true,
      fastModeSupportedModels: ["gpt-5.5", "gpt-5.4"],
      updatedAt: providerUpdatedAt,
    },
  ],
  defaultProvider: "codex",
  requiresOnboarding: false,
};

const globalRows: WorkspaceReviewRuntimeSettingsResponse[] = [
  {
    projectId: null,
    provider: "codex",
    model: "gpt-5.4",
    effort: "high",
    updatedAt: providerUpdatedAt,
  },
];

function openSelect(testId: string) {
  const trigger = screen.getByTestId(testId);
  fireEvent.keyDown(trigger, { key: "ArrowDown", code: "ArrowDown" });
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

describe("WorkspaceReviewSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(useAgentModels).mockReturnValue({
      models: [],
      registry: AGENT_MODEL_CATALOG,
      isReady: true,
      isLoading: false,
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
    } as ReturnType<typeof useAgentModels>);
    vi.mocked(useHarnessProviders).mockReturnValue({
      settings: enabledProviderSettings,
      providers: enabledProviderSettings.providers,
      isLoading: false,
      isPlaceholderData: false,
      isError: false,
      error: null,
      refetchProviders: vi.fn(),
      updateProviderAsync: vi.fn(),
      isUpdating: false,
      updateError: null,
    } as ReturnType<typeof useHarnessProviders>);
    vi.mocked(useReviewSettings).mockReturnValue({
      data: {
        require_human_review: false,
        require_workspace_review: true,
        max_fix_attempts: 3,
        max_revision_cycles: 5,
        ai_review_enabled: true,
        ai_review_auto_fix: true,
        require_fix_approval: false,
        auto_create_followup_agent_conversation: true,
        autofix_workspace_review_blocking_findings: true,
        workspace_review_fixer_cycle_cap: 3,
        run_task_validations: true,
      },
      isLoading: false,
    } as ReturnType<typeof useReviewSettings>);
    vi.mocked(useUpdateReviewSettings).mockReturnValue({
      mutate: updateReviewSettings,
      isPending: false,
    } as ReturnType<typeof useUpdateReviewSettings>);
    vi.mocked(useWorkspaceReviewRuntimeSettings).mockImplementation(
      (projectId) => ({
        rows: projectId === null ? [] : [],
        isLoading: false,
        isPlaceholderData: false,
        isError: false,
        error: null,
        updateSettings: updateRuntimeSettings,
        isUpdating: false,
        saveError: null,
      }),
    );
    vi.mocked(useProjectStore).mockReturnValue({
      id: "project-1",
      name: "Project One",
    });
    vi.mocked(useUiStore).mockImplementation((selector) =>
      selector({ openModal }),
    );
  });

  it("renders utility defaults for enabled providers and updates the publish gate", async () => {
    const user = userEvent.setup();
    render(<WorkspaceReviewSection />);

    expect(screen.getByText("Effective: haiku · Medium")).toBeInTheDocument();
    expect(
      screen.getByText("Effective: gpt-5.4-mini · Medium"),
    ).toBeInTheDocument();

    await user.click(
      screen.getByTestId("workspace-review-require-before-publish"),
    );

    expect(updateReviewSettings).toHaveBeenCalledWith({
      requireWorkspaceReview: false,
    });

    await user.click(
      screen.getByTestId("workspace-review-autofix-blocking-findings"),
    );

    expect(updateReviewSettings).toHaveBeenCalledWith({
      autofixWorkspaceReviewBlockingFindings: false,
    });

    const fixerCycleCap = screen.getByLabelText(
      "Maximum automatic fixer cycles",
    );
    expect(fixerCycleCap).toHaveAttribute(
      "aria-describedby",
      "workspace-review-fixer-cycle-cap-desc",
    );
    expect(fixerCycleCap).toHaveAttribute("min", "0");
    expect(
      screen.getByText(/Set 0 to turn off automatic fixing/),
    ).toBeInTheDocument();

    fireEvent.change(fixerCycleCap, { target: { value: "0" } });

    expect(updateReviewSettings).toHaveBeenLastCalledWith({
      workspaceReviewFixerCycleCap: 0,
    });
  });

  it("updates the global provider model and clears the explicit effort", async () => {
    const user = userEvent.setup();
    render(<WorkspaceReviewSection />);

    openSelect("workspace-review-model-codex");
    await user.click(await screen.findByText("gpt-5.4"));

    await waitFor(() =>
      expect(updateRuntimeSettings).toHaveBeenCalledWith(
        { provider: "codex", model: "gpt-5.4", effort: null },
        expect.any(Object),
      ),
    );
  });

  it("renders project overrides against global rows and saves to the project scope", async () => {
    const user = userEvent.setup();
    vi.mocked(useWorkspaceReviewRuntimeSettings).mockImplementation(
      (projectId) => ({
        rows: projectId === null ? globalRows : [],
        isLoading: false,
        isPlaceholderData: false,
        isError: false,
        error: null,
        updateSettings: updateRuntimeSettings,
        isUpdating: false,
        saveError: null,
      }),
    );

    render(<WorkspaceReviewSection />);
    await user.click(screen.getByRole("tab", { name: "Project Overrides" }));

    expect(screen.getByText("Effective: gpt-5.4 · High")).toBeInTheDocument();

    openSelect("workspace-review-effort-codex");
    const mediumOptions = await screen.findAllByText("Medium");
    await user.click(mediumOptions[mediumOptions.length - 1]!);

    await waitFor(() =>
      expect(updateRuntimeSettings).toHaveBeenCalledWith(
        { provider: "codex", model: null, effort: "medium" },
        expect.any(Object),
      ),
    );
    expect(useWorkspaceReviewRuntimeSettings).toHaveBeenCalledWith("project-1");
  });

  it("renders custom saved models in the model selector", async () => {
    vi.mocked(useWorkspaceReviewRuntimeSettings).mockImplementation(
      (projectId) => ({
        rows:
          projectId === null
            ? [
                {
                  projectId: null,
                  provider: "codex",
                  model: "gpt-local-review",
                  effort: null,
                  updatedAt: providerUpdatedAt,
                },
              ]
            : [],
        isLoading: false,
        isPlaceholderData: false,
        isError: false,
        error: null,
        updateSettings: updateRuntimeSettings,
        isUpdating: false,
        saveError: null,
      }),
    );

    render(<WorkspaceReviewSection />);

    expect(
      screen.getByText("Effective: gpt-local-review · Medium"),
    ).toBeInTheDocument();
    openSelect("workspace-review-model-codex");

    expect(await screen.findByText("Custom model")).toBeInTheDocument();
    expect(screen.getAllByText("gpt-local-review").length).toBeGreaterThan(0);
  });

  it("shows runtime save errors from provider rows", async () => {
    updateRuntimeSettings.mockImplementation((_input, options) => {
      options?.onError?.();
    });
    vi.mocked(useWorkspaceReviewRuntimeSettings).mockReturnValue({
      rows: [],
      isLoading: false,
      isPlaceholderData: false,
      isError: false,
      error: null,
      updateSettings: updateRuntimeSettings,
      isUpdating: false,
      saveError: new Error("Failed to save Workspace Review runtime"),
    } as ReturnType<typeof useWorkspaceReviewRuntimeSettings>);

    const user = userEvent.setup();
    render(<WorkspaceReviewSection />);

    openSelect("workspace-review-model-codex");
    await user.click(await screen.findByText("gpt-5.4"));

    expect(
      await screen.findByText("Failed to save Workspace Review runtime"),
    ).toBeInTheDocument();
  });

  it("shows provider setup notice and opens provider settings", async () => {
    const user = userEvent.setup();
    vi.mocked(useHarnessProviders).mockReturnValue({
      settings: {
        providers: [],
        defaultProvider: null,
        requiresOnboarding: true,
      },
      providers: [],
      isLoading: false,
      isPlaceholderData: false,
      isError: false,
      error: null,
      refetchProviders: vi.fn(),
      updateProviderAsync: vi.fn(),
      isUpdating: false,
      updateError: null,
    } as ReturnType<typeof useHarnessProviders>);

    render(<WorkspaceReviewSection />);

    expect(screen.getByText("Provider Setup Required")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /open providers/i }));

    expect(openModal).toHaveBeenCalledWith("settings", {
      section: "providers",
    });
  });

  it("shows disabled project override copy when no project is active", async () => {
    const user = userEvent.setup();
    vi.mocked(useProjectStore).mockReturnValue(null);

    render(<WorkspaceReviewSection />);
    await user.click(screen.getByRole("tab", { name: "Project Overrides" }));

    expect(
      screen.getByText(
        "Select a project to override its legacy Workspace Review fallback.",
      ),
    ).toBeInTheDocument();
  });
});
