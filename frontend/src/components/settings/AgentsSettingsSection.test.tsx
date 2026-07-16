import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ManualRoleCatalogEntry } from "@/api/manual-role-defaults.types";

import { AgentsSettingsSection } from "./AgentsSettingsSection";

const clearDefault = vi.fn();
const updateDefault = vi.fn();
const testState = vi.hoisted(() => ({
  activeProject: null as { id: string; name: string } | null,
  requestedScopes: [] as Array<string | null>,
  isLoading: false,
}));

vi.mock("@/hooks/useManualRoleDefaults", () => ({
  useManualRoleDefaults: (projectId: string | null) => {
    testState.requestedScopes.push(projectId);
    return {
    catalog: {
      projectId: null,
      roles: roleFixtures,
    },
    isLoading: testState.isLoading,
    isError: false,
    error: null,
    isSaving: false,
    updateDefault,
    clearDefault,
    };
  },
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
    family,
    familyDisplayName,
    configured: index === 0 ? defaultValue : null,
    effective: defaultValue,
    source: index === 0 ? "global_ui" : "provider_default",
    diagnostics: [],
    controls: {
      capabilities: [
        { value: "solo", enabled: true, disabledReason: null },
        {
          value: "rx_native_team",
          enabled: false,
          disabledReason: "Team is available only for Workspace root roles",
        },
      ],
      speeds: [
        { value: "provider_default", enabled: true, disabledReason: null },
        { value: "standard", enabled: true, disabledReason: null },
        {
          value: "fast",
          enabled: false,
          disabledReason: "Fast requires a supported Codex provider and model",
        },
      ],
      persona: {
        enabled: false,
        disabledReason: "Persona is limited to Workspace Project conversations in V1",
      },
    },
  }),
);

function renderSection() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <AgentsSettingsSection />
    </QueryClientProvider>,
  );
}

describe("AgentsSettingsSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    testState.activeProject = null;
    testState.requestedScopes.length = 0;
    testState.isLoading = false;
  });

  it("renders the seven backend-returned families without a frontend role catalog", () => {
    renderSection();

    for (const [, familyLabel] of families) {
      expect(
        screen.getByRole("button", {
          name: new RegExp(`^${familyLabel} \\(1\\)$`),
        }),
      ).toBeInTheDocument();
    }
    expect(screen.getAllByTestId("manual-role-row")).toHaveLength(roleFixtures.length);
    expect(screen.getByText(/Manual default · Global UI/)).toBeInTheDocument();
  });

  it("paints the Agents shell and loading placeholder before catalog rows", () => {
    testState.isLoading = true;
    renderSection();

    expect(screen.getByRole("heading", { name: "Agents" })).toBeInTheDocument();
    expect(screen.getByTestId("agents-settings-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("manual-role-row")).not.toBeInTheDocument();
  });

  it("follows the next source by clearing only the configured role row", async () => {
    const user = userEvent.setup();
    renderSection();

    await user.click(screen.getByRole("button", { name: "Follow Workspace role default" }));

    expect(clearDefault).toHaveBeenCalledWith("workspace_role_0");
  });

  it("loads project-scoped overrides from the active project tab", async () => {
    const user = userEvent.setup();
    testState.activeProject = { id: "project-1", name: "RalphX" };
    renderSection();

    await user.click(screen.getByRole("tab", { name: "Project Overrides" }));

    expect(testState.requestedScopes).toContain("project-1");
    expect(screen.getByText(/Overrides for RalphX/)).toBeInTheDocument();
  });

  it("writes one complete Manual default when a role control changes", async () => {
    const user = userEvent.setup();
    renderSection();

    await user.selectOptions(
      screen.getByRole("combobox", { name: "Workspace role provider" }),
      "codex",
    );

    expect(updateDefault).toHaveBeenCalledWith("workspace_role_0", {
      ...defaultValue,
      provider: "codex",
      model: "gpt-5.6",
      effort: "xhigh",
    });
  });

  it("shows backend-owned disabled reasons for capability, speed, and persona", () => {
    renderSection();

    expect(
      screen.getAllByText("Team is available only for Workspace root roles").length,
    ).toBeGreaterThan(0);
    expect(
      screen.getAllByText("Fast requires a supported Codex provider and model").length,
    ).toBeGreaterThan(0);
    expect(
      screen.getAllByText("Persona is limited to Workspace Project conversations in V1").length,
    ).toBeGreaterThan(0);
  });
});
