import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ManualRoleCatalogEntry } from "@/api/manual-role-defaults.types";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useAgentSessionStore } from "@/stores/agentSessionStore";

import { AgentsSettingsSection } from "./AgentsSettingsSection";

const afterPaintCallbacks = vi.hoisted(() => [] as Array<() => void>);
const clearDefaultAsync = vi.fn().mockResolvedValue(true);
const dismissSaveError = vi.fn();
const updateDefault = vi.fn();
const testState = vi.hoisted(() => ({
  activeProject: null as { id: string; name: string } | null,
  requestedScopes: [] as Array<string | null>,
  isHostOnly: false,
  isLoading: false,
  error: null as unknown,
  saveError: null as unknown,
  isSaving: false,
  roles: [] as ManualRoleCatalogEntry[],
  tasksFeatureState: "enabled" as "enabled" | "draining" | "disabled",
}));

vi.mock("./SettingsDialog.performance", () => ({
  scheduleAfterPaint: (callback: () => void) => {
    afterPaintCallbacks.push(callback);
    return { frame: null, timer: null };
  },
}));

vi.mock("@/hooks/useManualRoleDefaults", () => ({
  useManualRoleDefaults: (projectId: string | null) => {
    testState.requestedScopes.push(projectId);
    return {
      catalog: {
        projectId,
        roles: testState.roles,
      },
      isHostOnly: testState.isHostOnly,
      isLoading: testState.isLoading,
      isError: testState.error !== null,
      error: testState.error,
      saveError: testState.saveError,
      dismissSaveError,
      isSaving: testState.isSaving,
      updateDefault,
      clearDefaultAsync,
    };
  },
}));

vi.mock("@/hooks/useIdeationSettings", () => ({
  useIdeationSettings: () => ({
    settings: {
      tasksEnabled: testState.tasksFeatureState === "enabled",
      tasksFeatureState: testState.tasksFeatureState,
    },
    isLoading: false,
    isError: false,
  }),
}));

vi.mock("@/hooks/useAgentModels", () => ({
  useAgentModels: () => ({
    registry: {
      claude: [{ id: "sonnet", label: "Sonnet", menuLabel: "Sonnet", defaultEffort: "high", supportedEfforts: ["high"] }],
      codex: [{ id: "gpt-5.6", label: "GPT-5.6", menuLabel: "GPT-5.6", defaultEffort: "xhigh", supportedEfforts: ["xhigh"] }],
    },
    isReady: true,
  }),
}));

vi.mock("@/hooks/usePersonas", () => ({
  fetchPersonas: vi.fn().mockResolvedValue([]),
  personaKeys: { list: () => ["personas", "list"] },
}));

vi.mock("@/hooks/useFeatureFlags", () => ({
  useFeatureFlags: () => ({ data: { agentPersonas: true } }),
}));

vi.mock("@/hooks/useHarnessProviders", () => ({
  useHarnessProviders: () => ({ providers: [] }),
}));

vi.mock("@/stores/projectStore", () => ({
  selectActiveProject: () => testState.activeProject,
  useProjectStore: (selector: (state: object) => unknown) => selector({}),
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: (selector: (state: { openModal: ReturnType<typeof vi.fn> }) => unknown) =>
    selector({ openModal: vi.fn() }),
}));

const families = [
  ["workspace", "Workspace"],
  ["automation", "Automation"],
  ["feedback_loops", "Feedback Loops"],
  ["ideation", "Ideation"],
  ["delegation", "Delegation"],
  ["execution", "Execution"],
  ["utility", "Utility"],
] as const;

const defaultValue = {
  provider: "claude",
  model: "sonnet",
  effort: "high",
  serviceTier: "provider_default" as const,
  coordinationMode: null,
  personaId: null,
  approvalPolicy: null,
  sandboxMode: null,
};

const roleFixtures: ManualRoleCatalogEntry[] = families.map(
  ([family, familyDisplayName], index) => ({
    role: `${family}_role_${index}`,
    displayName: `${familyDisplayName} role`,
    description: `Handles ${familyDisplayName.toLowerCase()} work.`,
    family,
    familyDisplayName,
    requiresTasks: family === "execution",
    configured: index === 0 ? defaultValue : null,
    effective: defaultValue,
    source: index === 0 ? "global_ui" : "provider_default",
    diagnostics: [],
    controls: {
      capabilities: [
        { value: "solo", enabled: true, disabledReason: null },
        { value: "rx_native_team", enabled: false, disabledReason: "Team is unavailable" },
      ],
      speeds: [
        { value: "provider_default", enabled: true, disabledReason: null },
        { value: "fast", enabled: false, disabledReason: "Fast is unavailable" },
      ],
      persona: { enabled: false, disabledReason: "Persona is unavailable" },
    },
  }),
);

function renderSection() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <AgentsSettingsSection />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

function flushAfterPaint() {
  const callbacks = afterPaintCallbacks.splice(0);
  callbacks.forEach((callback) => callback());
}

describe("AgentsSettingsSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    afterPaintCallbacks.length = 0;
    localStorage.clear();
    useAgentSessionStore.setState({ defaultStartMode: "edit" });
    testState.activeProject = null;
    testState.requestedScopes.length = 0;
    testState.isLoading = false;
    testState.error = null;
    testState.saveError = null;
    testState.isSaving = false;
    testState.roles = roleFixtures;
    testState.tasksFeatureState = "enabled";
  });

  it("explains that role defaults are host-only instead of blaming the filters", () => {
    // The host does not expose `get_manual_role_defaults`, so a paired client used to render
    // a raw REMOTE_COMMAND_UNAVAILABLE banner. With the read suppressed the catalog is empty,
    // which must NOT fall through to the "no roles match these filters" empty state.
    testState.isHostOnly = true;
    testState.roles = [];

    renderSection();

    expect(screen.getByTestId("remote-host-only-notice")).toBeInTheDocument();
    expect(
      screen.queryByText("No agent roles match these filters."),
    ).not.toBeInTheDocument();

    testState.isHostOnly = false;
  });

  it("renders seven collapsed backend-returned family overviews without mounting role editors", () => {
    renderSection();

    for (const [, familyLabel] of families) {
      expect(screen.getByRole("button", { name: new RegExp(`^${familyLabel}`) }))
        .toHaveAttribute("aria-expanded", "false");
    }
    expect(screen.queryByTestId("manual-role-row")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("combobox", { name: "Workspace role provider" }),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("agent-default-start-mode")).toBeInTheDocument();
    expect(screen.getByText("1 configured")).toBeInTheDocument();
  });

  it("changes the default new-run mode through the radio card group", async () => {
    renderSection();

    fireEvent.click(screen.getByRole("radio", { name: /Plan/ }));

    await waitFor(() => {
      expect(useAgentSessionStore.getState().defaultStartMode).toBe("plan");
    });
  });

  it("hides Tasks-only execution roles without deleting their configured overrides", () => {
    testState.tasksFeatureState = "disabled";
    testState.roles = roleFixtures.map((role) =>
      role.family === "execution" ? { ...role, configured: defaultValue } : role,
    );

    const { rerender } = renderSection();

    expect(screen.queryByRole("button", { name: /^Execution/ })).not.toBeInTheDocument();
    expect(screen.getByTestId("tasks-required-roles-hidden-notice")).toHaveTextContent(
      "Saved overrides are preserved",
    );
    expect(testState.roles.find((role) => role.family === "execution")?.configured)
      .toEqual(defaultValue);

    testState.tasksFeatureState = "enabled";
    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <TooltipProvider>
          <AgentsSettingsSection />
        </TooltipProvider>
      </QueryClientProvider>,
    );
    expect(screen.getByRole("button", { name: /^Execution/ })).toBeInTheDocument();
  });

  it("opens a family into compact role summaries and mounts controls only after role disclosure", async () => {
    const user = userEvent.setup();
    renderSection();

    await user.click(screen.getByRole("button", { name: /^Workspace/ }));
    expect(screen.getAllByTestId("manual-role-row")).toHaveLength(1);
    expect(screen.queryByRole("button", { name: /^Runtime:/ }))
      .not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Edit Workspace role" }));
    expect(screen.getByRole("button", { name: /^Runtime:/ }))
      .toBeInTheDocument();
  });

  it("updates disclosure before deferring persistence until after paint", async () => {
    const user = userEvent.setup();
    const setItem = vi.spyOn(Storage.prototype, "setItem");
    renderSection();

    const workspace = screen.getByRole("button", { name: /^Workspace/ });
    await user.click(workspace);

    expect(workspace).toHaveAttribute("aria-expanded", "true");
    expect(setItem).not.toHaveBeenCalled();
    expect(afterPaintCallbacks).toHaveLength(1);

    flushAfterPaint();
    expect(setItem).toHaveBeenCalled();
  });

  it("forces matching families visible without overwriting their saved disclosure", async () => {
    const user = userEvent.setup();
    renderSection();

    const workspace = screen.getByRole("button", { name: /^Workspace/ });
    expect(workspace).toHaveAttribute("aria-expanded", "false");
    await user.click(workspace);
    expect(workspace).toHaveAttribute("aria-expanded", "true");

    const search = screen.getByRole("searchbox", { name: "Search agent roles" });
    await user.type(search, "workspace work");
    expect(workspace).toHaveAttribute("aria-expanded", "true");
    expect(screen.getAllByTestId("manual-role-row")).toHaveLength(1);
    await user.click(workspace);

    await user.clear(search);
    expect(workspace).toHaveAttribute("aria-expanded", "true");
    expect(screen.getAllByTestId("manual-role-row")).toHaveLength(1);
  });

  it("filters configured overrides and attention states from backend catalog values", async () => {
    const user = userEvent.setup();
    testState.roles = roleFixtures.map((role, index) =>
      index === 1 ? { ...role, diagnostics: ["Needs repair"] } : role,
    );
    renderSection();

    await user.click(screen.getByRole("button", { name: "Overrides only" }));
    expect(screen.getAllByTestId("manual-role-row")).toHaveLength(1);
    expect(screen.getByText("Workspace role")).toBeInTheDocument();
    expect(screen.queryByText("Automation role")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Overrides only" }));
    await user.click(screen.getByRole("button", { name: "Needs attention" }));
    expect(screen.getAllByTestId("manual-role-row")).toHaveLength(1);
    expect(screen.getByText("Automation role")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Needs repair");
    expect(screen.getByRole("button", { name: "Permissions" }))
      .toHaveAttribute("aria-expanded", "true");
  });

  it("keeps global and project disclosure independent and restores the saved tab", async () => {
    const user = userEvent.setup();
    testState.activeProject = { id: "project-1", name: "RalphX" };
    const { unmount } = renderSection();

    await user.click(screen.getByRole("button", { name: /^Workspace/ }));
    await user.click(screen.getByRole("tab", { name: "Project Overrides" }));
    expect(screen.getByRole("button", { name: /^Workspace/ }))
      .toHaveAttribute("aria-expanded", "false");
    await user.click(screen.getByRole("button", { name: /^Automation/ }));
    expect(testState.requestedScopes).toContain("project-1");

    flushAfterPaint();

    unmount();
    renderSection();
    expect(screen.getByRole("tab", { name: "Project Overrides" }))
      .toHaveAttribute("data-state", "active");
    expect(screen.getByRole("button", { name: /^Workspace/ }))
      .toHaveAttribute("aria-expanded", "false");
    expect(screen.getByRole("button", { name: /^Automation/ }))
      .toHaveAttribute("aria-expanded", "true");
  });

  it("restores a saved project tab when the active project hydrates later", async () => {
    localStorage.setItem(
      "ralphx-settings-agents-state",
      JSON.stringify({ version: 1, activeTab: "project", disclosures: {} }),
    );
    const rendered = renderSection();
    expect(screen.getByRole("tab", { name: "Global Defaults" }))
      .toHaveAttribute("data-state", "active");

    testState.activeProject = { id: "project-1", name: "Project One" };
    rendered.rerender(
      <QueryClientProvider client={new QueryClient()}>
        <TooltipProvider>
          <AgentsSettingsSection />
        </TooltipProvider>
      </QueryClientProvider>,
    );

    await waitFor(() => {
      expect(screen.getByRole("tab", { name: "Project Overrides" }))
        .toHaveAttribute("data-state", "active");
    });
  });

  it("disables scope changes while a mutation is pending and shows dismissible save failures", () => {
    testState.isSaving = true;
    testState.saveError = new Error("Could not save role");
    renderSection();

    expect(screen.getByRole("tab", { name: "Global Defaults" })).toBeDisabled();
    expect(screen.getByRole("tab", { name: "Project Overrides" })).toBeDisabled();
    expect(screen.getByText("Could not save role")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Dismiss error" })).toBeInTheDocument();
  });

  it("clears a settled mutation failure when the user changes scope", async () => {
    const user = userEvent.setup();
    testState.activeProject = { id: "project-1", name: "Project One" };
    testState.saveError = new Error("Could not save role");
    renderSection();

    await user.click(screen.getByRole("tab", { name: "Project Overrides" }));

    expect(dismissSaveError).toHaveBeenCalledOnce();
  });

  it("keeps load failures distinct from empty filtered results", async () => {
    const user = userEvent.setup();
    testState.error = new Error("Catalog unavailable");
    renderSection();

    expect(screen.getByText("Catalog unavailable")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Dismiss error" })).not.toBeInTheDocument();
    await user.type(screen.getByRole("searchbox", { name: "Search agent roles" }), "missing");
    expect(screen.queryByText("No agent roles match these filters.")).not.toBeInTheDocument();
  });

  it("paints the Agents shell and loading placeholder before catalog rows", () => {
    testState.isLoading = true;
    renderSection();

    expect(screen.getByTestId("agent-default-start-mode")).toBeInTheDocument();
    expect(screen.getByTestId("agents-settings-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("agent-family-row")).not.toBeInTheDocument();
  });
});
