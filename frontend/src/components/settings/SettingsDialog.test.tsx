/**
 * SettingsDialog component tests
 *
 * Tests for the modal-based Settings Dialog:
 * - Opens when activeModal === "settings"
 * - Closes via close button / Escape / backdrop
 * - Deep-link section init from modalContext.section
 * - Section switching via left rail
 * - All settings sections render without throwing
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render as rtlRender, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import SettingsDialog from "./SettingsDialog";
import { invoke } from "@tauri-apps/api/core";
import { DEFAULT_PROJECT_SETTINGS } from "@/types/settings";
import {
  SETTINGS_NAV,
  SETTINGS_SECTIONS,
  isSettingsSectionVisible,
  navForSection,
  sectionMeta,
} from "./settings-registry";
import { sectionModuleLoaders } from "./SettingsDialog.performance";

const featureFlags = vi.hoisted(() => ({ agentPersonas: false }));
const settingsTasksEnabledRef = vi.hoisted(() => ({ current: true }));

vi.mock("@/hooks/useIdeationSettings", () => ({
  useIdeationSettings: () => ({
    settings: {
      tasksEnabled: settingsTasksEnabledRef.current,
      requireAcceptForFinalize: true,
      autoVerifyPlans: false,
      requireVerificationForAccept: false,
      externalOverrides: {
        autoVerifyPlans: null,
        requireVerificationForAccept: null,
        requireAcceptForFinalize: null,
      },
    },
    updateSettings: vi.fn(),
    isLoading: false,
    isError: false,
    isUpdating: false,
    updateError: null,
  }),
}));

// ---------------------------------------------------------------------------
// uiStore mock
// ---------------------------------------------------------------------------

const mockCloseModal = vi.fn();

const uiState = vi.hoisted(() => ({
  activeModal: null as string | null,
  modalContext: undefined as Record<string, unknown> | undefined,
  closeModal: vi.fn(),
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: (selector: (s: typeof uiState) => unknown) => selector(uiState),
}));

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({ subscribe: vi.fn(() => vi.fn()) }),
}));

vi.mock("@/hooks/useFeatureFlags", () => ({
  useFeatureFlags: () => ({ data: featureFlags }),
}));

vi.mock("./sections/GlobalExecutionSection", () => ({
  default: () => <div data-testid="global-execution-section">Global Capacity</div>,
}));

vi.mock("./sections/ReviewPolicySection", () => ({
  default: () => <div data-testid="review-policy-section">Review Policy</div>,
}));

vi.mock("./sections/WorkspaceReviewSection", () => ({
  default: () => (
    <div data-testid="workspace-review-section">Workspace Review</div>
  ),
}));

vi.mock("./sections/AutonomyPolicySection", () => ({
  default: () => (
    <div data-testid="autonomy-policy-section">Autonomy Policy</div>
  ),
}));

vi.mock("./sections/TasksSettingsSection", () => ({
  default: ({ initialTab }: { initialTab: string }) => (
    <div data-testid="tasks-section">Tasks {initialTab}</div>
  ),
}));

vi.mock("./sections/PlanningSettingsSection", () => ({
  default: () => <div data-testid="planning-section">Planning</div>,
}));

vi.mock("./sections/WorkspaceSettingsSection", () => ({
  default: ({ initialTab }: { initialTab: string }) => (
    <div data-testid="workspace-section">Workspace {initialTab}</div>
  ),
}));

vi.mock("./sections/CapacitySettingsSection", () => ({
  default: () => <div data-testid="capacity-section">Capacity</div>,
}));

vi.mock("./ExternalMcpSettingsPanel", () => ({
  ExternalMcpSettingsPanel: () => (
    <div data-testid="external-mcp-section">External MCP</div>
  ),
}));

vi.mock("./McpSettingsSection", () => ({
  McpSettingsSection: () => <div data-testid="mcp-section">MCP</div>,
}));

vi.mock("./UpdatesSettingsSection", () => ({
  UpdatesSettingsSection: () => <div data-testid="updates-section">Updates</div>,
}));

vi.mock("./HarnessProvidersSection", () => ({
  HarnessProvidersSection: () => (
    <div data-testid="providers-section">Providers</div>
  ),
}));

vi.mock("./RepositorySettingsSection", () => ({
  RepositorySettingsSection: () => (
    <div data-testid="repository-section">Repository</div>
  ),
}));

vi.mock("./PersonasSection", () => ({
  PersonasSection: () => <div data-testid="personas-section">Personas</div>,
}));

vi.mock("./CapabilitiesSection", () => ({
  CapabilitiesSection: () => (
    <div data-testid="capabilities-section">Capabilities</div>
  ),
}));

vi.mock("./AgentsSettingsSection", () => ({
  AgentsSettingsSection: () => <div data-testid="agents-section">Agents</div>,
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const createTestQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  });

const defaultProps = {
  executionSettings: DEFAULT_PROJECT_SETTINGS,
  isLoadingSettings: false,
  isSavingSettings: false,
  settingsError: null,
  onSettingsChange: vi.fn(),
};

const render = (ui: React.ReactElement) =>
  rtlRender(
    <QueryClientProvider client={createTestQueryClient()}>{ui}</QueryClientProvider>
  );

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("SettingsDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    uiState.activeModal = null;
    uiState.modalContext = undefined;
    uiState.closeModal = mockCloseModal;
    featureFlags.agentPersonas = false;
    settingsTasksEnabledRef.current = true;
    vi.mocked(invoke).mockResolvedValue(undefined);
  });

  // --------------------------------------------------------------------------
  // Open / close
  // --------------------------------------------------------------------------

  describe("Open/Close behavior", () => {
    it("renders dialog when activeModal is 'settings'", () => {
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);
      expect(screen.getByTestId("settings-dialog")).toBeInTheDocument();
    });

    it("does not render dialog content when activeModal is null", () => {
      uiState.activeModal = null;
      render(<SettingsDialog {...defaultProps} />);
      expect(screen.queryByTestId("settings-dialog")).not.toBeInTheDocument();
    });

    it("does not render dialog content when activeModal is a different type", () => {
      uiState.activeModal = "task-create";
      render(<SettingsDialog {...defaultProps} />);
      expect(screen.queryByTestId("settings-dialog")).not.toBeInTheDocument();
    });

    it("calls closeModal when close button is clicked", async () => {
      const user = userEvent.setup();
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      await user.click(screen.getByRole("button", { name: "Close settings" }));

      expect(screen.getByTestId("settings-dialog")).toHaveClass("opacity-0");
      await waitFor(() => expect(mockCloseModal).toHaveBeenCalledTimes(1));
    });
  });

  // --------------------------------------------------------------------------
  // currentView is NOT changed when opening settings
  // --------------------------------------------------------------------------

  describe("currentView independence", () => {
    it("opening settings does not require or change currentView", () => {
      // SettingsDialog only reads activeModal/modalContext — not currentView.
      // Verifying the dialog renders based solely on activeModal === "settings".
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);
      // Dialog is open; currentView is NOT part of the dialog's state
      expect(screen.getByTestId("settings-dialog")).toBeInTheDocument();
    });
  });

  // --------------------------------------------------------------------------
  // Deep-link section initialization
  // --------------------------------------------------------------------------

  it("registers Personas as a leaf of the Agents nav entry", () => {
    expect(sectionMeta("personas")?.label).toBe("Personas");
    expect(navForSection("personas").id).toBe("agents");
  });

  it("keeps Personas available through the deferred settings-section loader", async () => {
    await expect(sectionModuleLoaders.personas()).resolves.toHaveProperty("PersonasSection");
  });

  it("registers and defers the Capabilities settings section", async () => {
    expect(sectionMeta("capabilities")?.label).toBe("Capabilities");
    expect(navForSection("capabilities").id).toBe("agents");
    await expect(sectionModuleLoaders.capabilities()).resolves.toHaveProperty(
      "CapabilitiesSection",
    );
  });

  it("registers Updates under Application and keeps its module lazy", async () => {
    expect(sectionMeta("updates")?.label).toBe("Updates");
    expect(navForSection("updates").id).toBe("application");
    await expect(sectionModuleLoaders.updates()).resolves.toHaveProperty(
      "UpdatesSettingsSection",
    );

    uiState.activeModal = "settings";
    uiState.modalContext = { section: "updates" };
    render(<SettingsDialog {...defaultProps} />);

    expect(await screen.findByTestId("updates-section")).toBeInTheDocument();
  });

  it("consolidates the legacy agent pages into one lazy Agents section", async () => {
    expect(sectionMeta("agents")?.label).toBe("Roles");
    expect(navForSection("agents").id).toBe("agents");
    expect(SETTINGS_SECTIONS.some((section) => section.id === "execution-harnesses")).toBe(false);
    expect(SETTINGS_SECTIONS.some((section) => section.id === "ideation-harnesses")).toBe(false);
    await expect(sectionModuleLoaders.agents()).resolves.toHaveProperty(
      "AgentsSettingsSection",
    );
  });

  it("registers External MCP under Integrations and preserves the deep link", async () => {
    expect(navForSection("mcp").id).toBe("models-providers");
    expect(sectionMeta("external-mcp")?.label).toBe("External MCP");
    expect(navForSection("external-mcp").id).toBe("integrations");
    await expect(sectionModuleLoaders.mcp()).resolves.toHaveProperty("McpSettingsSection");

    uiState.activeModal = "settings";
    uiState.modalContext = { section: "external-mcp" };
    render(<SettingsDialog {...defaultProps} />);
    expect(await screen.findByTestId("external-mcp-section")).toBeInTheDocument();
  });

  describe("Section initialization via modalContext deep-link", () => {
    it("defaults to the Providers section when no modalContext.section is provided", async () => {
      uiState.activeModal = "settings";
      uiState.modalContext = undefined;
      render(<SettingsDialog {...defaultProps} />);

      expect(await screen.findByTestId("providers-section")).toBeInTheDocument();
    });

    it("initializes to API Keys section when modalContext.section is 'api-keys'", () => {
      uiState.activeModal = "settings";
      uiState.modalContext = { section: "api-keys" };
      render(<SettingsDialog {...defaultProps} />);

      // API Keys section is active — breadcrumb shows "API Keys" (appears in nav rail + breadcrumb)
      const apiKeysTexts = screen.getAllByText("API Keys");
      expect(apiKeysTexts.length).toBeGreaterThanOrEqual(1);
      // Execution section content should NOT be rendered (only active section renders)
      expect(screen.queryByTestId("max-concurrent-tasks")).not.toBeInTheDocument();
    });

    it("routes the legacy Execution Agents deep link into Agents", async () => {
      uiState.activeModal = "settings";
      uiState.modalContext = { section: "execution-harnesses" };
      render(<SettingsDialog {...defaultProps} />);

      expect(await screen.findByTestId("agents-section")).toBeInTheDocument();
      expect(screen.getByText("Agents", { selector: ".cur" })).toBeInTheDocument();
    });

    it("routes the legacy Ideation Agents deep link into Agents", async () => {
      uiState.activeModal = "settings";
      uiState.modalContext = { section: "ideation-harnesses" };
      render(<SettingsDialog {...defaultProps} />);

      expect(await screen.findByTestId("agents-section")).toBeInTheDocument();
    });

    it.each([
      ["review", "tasks-section", "review-policy"],
      ["autonomy", "tasks-section", "autonomy-policy"],
      ["ideation-workflow", "tasks-section", "general"],
      ["workspace-review", "workspace-section", "review"],
      ["execution", "workspace-section", "general"],
      ["global-execution", "capacity-section", "Capacity"],
    ])("routes legacy %s to its parent and tab", async (section, testId, expected) => {
      uiState.activeModal = "settings";
      uiState.modalContext = {
        section,
        provider: "claude",
        scope: "project",
        serverId: "ralphx",
      };
      render(<SettingsDialog {...defaultProps} />);

      expect(await screen.findByTestId(testId)).toHaveTextContent(expected);
      expect(uiState.modalContext).toMatchObject({
        provider: "claude",
        scope: "project",
        serverId: "ralphx",
      });
    });

    it("opens the Personas section from a settings deep link when enabled", async () => {
      uiState.activeModal = "settings";
      uiState.modalContext = { section: "personas" };
      featureFlags.agentPersonas = true;
      render(<SettingsDialog {...defaultProps} />);

      expect(screen.getByTestId("settings-section-loading")).toBeInTheDocument();
      expect(screen.queryByTestId("personas-section")).not.toBeInTheDocument();
      expect(await screen.findByTestId("personas-section")).toBeInTheDocument();
    });

  });

  it("keeps Tasks, Planning, Workspace, and Capacity visible while Tasks are off", async () => {
    settingsTasksEnabledRef.current = false;
    uiState.activeModal = "settings";
    uiState.modalContext = { section: "execution" };
    render(<SettingsDialog {...defaultProps} />);

    for (const leaf of ["Tasks", "Planning", "Workspace", "Capacity"]) {
      expect(screen.getByRole("tab", { name: leaf })).toBeInTheDocument();
    }
    expect(await screen.findByTestId("workspace-section")).toHaveTextContent("general");
  });

  // --------------------------------------------------------------------------
  // Sidebar order / first paint / persisted section
  // --------------------------------------------------------------------------

  describe("Sidebar order and section persistence", () => {
    it("renders the seven consolidated nav entries in registry order", () => {
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      const navigation = screen.getByRole("navigation");
      expect(
        within(navigation)
          .getAllByRole("button")
          .map((element) => element.getAttribute("data-nav")),
      ).toEqual(SETTINGS_NAV.map((nav) => nav.id));
      expect(within(navigation).getAllByRole("button")[0]).toHaveAccessibleName(
        "Models & Providers",
      );
    });

    it("shows each nav entry's sublabel", () => {
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      const navigation = screen.getByRole("navigation");
      for (const nav of SETTINGS_NAV) {
        expect(within(navigation).getByText(nav.sublabel)).toBeInTheDocument();
      }
    });

    it("uses the v30 settings modal shell classes and active nav treatment", () => {
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      const dialog = screen.getByTestId("settings-dialog");
      expect(dialog).toHaveClass("settings-modal");

      const navigation = screen.getByRole("navigation");
      expect(navigation).toHaveClass("settings-nav");
      const active = within(navigation).getByRole("button", {
        name: "Models & Providers",
      });
      expect(active).toHaveClass("settings-nav__item");
      expect(active).toHaveAttribute("aria-current", "page");
    });

    it("renders the settings layer above global toasts", () => {
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      expect(screen.getByTestId("modal-overlay")).toHaveClass("z-50");
      expect(screen.getByTestId("settings-dialog")).toHaveClass("z-50");
    });

    it("paints the settings shell before hydrating the active section", () => {
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      expect(screen.getByTestId("settings-dialog")).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "Models & Providers" }),
      ).toBeInTheDocument();
      expect(screen.getByTestId("settings-page-title")).toHaveTextContent(
        "Models & Providers",
      );
      expect(screen.getByTestId("settings-section-loading")).toBeInTheDocument();
      expect(screen.queryByTestId("providers-section")).not.toBeInTheDocument();
    });

    it("keeps Personas navigation available when the feature flag is off", () => {
      uiState.activeModal = "settings";
      uiState.modalContext = { section: "personas" };
      featureFlags.agentPersonas = false;
      render(<SettingsDialog {...defaultProps} />);

      expect(screen.getByRole("tab", { name: "Personas" })).toBeInTheDocument();
      expect(screen.queryByTestId("personas-section")).not.toBeInTheDocument();
      expect(invoke).not.toHaveBeenCalledWith("list_personas", { input: {} });
    });

    it("remembers the last opened section across Settings reopens", async () => {
      const user = userEvent.setup();
      uiState.activeModal = "settings";
      const { rerender } = render(<SettingsDialog {...defaultProps} />);

      expect(await screen.findByTestId("providers-section")).toBeInTheDocument();

      await user.click(screen.getByRole("button", { name: "Automation" }));
      expect(await screen.findByTestId("tasks-section")).toBeInTheDocument();
      await waitFor(() =>
        expect(localStorage.getItem("ralphx-settings-active-section")).toBe(
          "tasks",
        ),
      );

      uiState.activeModal = null;
      rerender(<SettingsDialog {...defaultProps} />);

      uiState.activeModal = "settings";
      rerender(<SettingsDialog {...defaultProps} />);
      expect(await screen.findByTestId("tasks-section")).toBeInTheDocument();
    });
  });

  // --------------------------------------------------------------------------
  // Left rail section switching
  // --------------------------------------------------------------------------

  describe("Left rail section switching", () => {
    it("switches active section when a left rail item is clicked", async () => {
      const user = userEvent.setup();
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      expect(await screen.findByTestId("providers-section")).toBeInTheDocument();

      const automationNavItem = screen.getByRole("button", {
        name: "Automation",
      });
      await user.click(automationNavItem);

      expect(screen.queryByTestId("providers-section")).not.toBeInTheDocument();
      expect(await screen.findByTestId("tasks-section")).toBeInTheDocument();
    });

    it("switches active section via keyboard Enter on left rail item", async () => {
      const user = userEvent.setup();
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      const automationNavItem = screen.getByRole("button", {
        name: "Automation",
      });
      automationNavItem.focus();
      await user.keyboard("{Enter}");

      expect(await screen.findByTestId("tasks-section")).toBeInTheDocument();
    });
  });

  // --------------------------------------------------------------------------
  // All sections render without throwing
  // --------------------------------------------------------------------------

  describe("All sections render without throwing", () => {
    it.each(SETTINGS_SECTIONS)(
      "renders $label section ($id) without throwing",
      ({ id }) => {
        uiState.activeModal = "settings";
        uiState.modalContext = { section: id };
        expect(() => render(<SettingsDialog {...defaultProps} />)).not.toThrow();
      }
    );
  });

  // --------------------------------------------------------------------------
  // Nav consolidation must not strand any pre-existing leaf
  // --------------------------------------------------------------------------

  describe("Leaf reachability after nav consolidation", () => {
    /**
     * Every leaf id that existed before the seven-entry nav landed.
     *
     * Feature-flag-gated sections are excluded: with their flag off they are deliberately
     * absent from the nav and the hub, so "reachable" is not a claim that applies to them.
     * `isSettingsSectionVisible({})` is the same gate the dialog itself consults, so a
     * section that later loses its flag re-enters this list automatically.
     */
    const LEGACY_LEAF_IDS = SETTINGS_SECTIONS.map((s) => s.id).filter(
      (id) => id !== "integrations-hub" && isSettingsSectionVisible(id, {}),
    );

    it.each(LEGACY_LEAF_IDS)(
      "keeps %s reachable from its nav entry",
      async (id) => {
        const user = userEvent.setup();
        uiState.activeModal = "settings";
        render(<SettingsDialog {...defaultProps} />);

        const nav = navForSection(id);
        await user.click(screen.getByRole("button", { name: nav.label }));

        if (nav.hubLeaf === id) {
          expect(await screen.findByTestId("integrations-hub")).toBeInTheDocument();
          return;
        }

        if (nav.hubLeaf) {
          // Drill-in: reached from a hub card, and the back button returns.
          const card = await screen.findByTestId(`integration-card-${id}`);
          await user.click(within(card).getByRole("button"));
          await waitFor(() =>
            expect(
              screen.getByTestId("settings-drill-in-back"),
            ).toBeInTheDocument(),
          );
          await user.click(screen.getByTestId("settings-drill-in-back"));
          expect(await screen.findByTestId("integrations-hub")).toBeInTheDocument();
          return;
        }

        if (nav.leaves.length > 1) {
          const label = sectionMeta(id)?.label as string;
          await user.click(screen.getByRole("tab", { name: label }));
          expect(screen.getByRole("tab", { name: label })).toHaveAttribute(
            "aria-selected",
            "true",
          );
        }

        await waitFor(() =>
          expect(localStorage.getItem("ralphx-settings-active-section")).toBe(id),
        );
      },
    );

    it.each(LEGACY_LEAF_IDS)(
      "still honours the openModal deep link for %s",
      async (id) => {
        uiState.activeModal = "settings";
        uiState.modalContext = { section: id };
        render(<SettingsDialog {...defaultProps} />);

        // The legacy flat "integrations" id now lands on the hub by design;
        // every other id restores its own leaf.
        const expected = id === "integrations" ? "integrations-hub" : id;
        await waitFor(() =>
          expect(localStorage.getItem("ralphx-settings-active-section")).toBe(
            expected,
          ),
        );
        expect(screen.getByTestId("settings-dialog")).toBeInTheDocument();
      },
    );

    it("reaches the Atlassian panel through the explicit alias", async () => {
      uiState.activeModal = "settings";
      uiState.modalContext = { section: "atlassian" };
      render(<SettingsDialog {...defaultProps} />);

      await waitFor(() =>
        expect(localStorage.getItem("ralphx-settings-active-section")).toBe(
          "integrations",
        ),
      );
      expect(screen.getByTestId("settings-drill-in-back")).toBeInTheDocument();
    });

    it("navigates and persists when a search result carries a composite tab", async () => {
      const user = userEvent.setup();
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      await user.type(
        screen.getByRole("searchbox", { name: "Search settings" }),
        "review policy",
      );
      await user.click(screen.getByRole("option", { name: /review policy/i }));

      // The composite tab rides along in state while persistence stores the
      // leaf id, exactly as a nav click would.
      await waitFor(() =>
        expect(localStorage.getItem("ralphx-settings-active-section")).toBe(
          "tasks",
        ),
      );
      expect(
        within(screen.getByTestId("settings-nav-automation")).getByText("Automation"),
      ).toBeInTheDocument();
      expect(screen.getByTestId("settings-nav-automation")).toHaveAttribute(
        "aria-current",
        "page",
      );
    });

    it("navigates from a tabless search result", async () => {
      const user = userEvent.setup();
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      await user.type(
        screen.getByRole("searchbox", { name: "Search settings" }),
        "granola",
      );
      const [firstResult] = screen.getAllByRole("option");
      await user.click(firstResult as HTMLElement);

      await waitFor(() =>
        expect(localStorage.getItem("ralphx-settings-active-section")).toBe(
          "granola",
        ),
      );
    });
  });

  // --------------------------------------------------------------------------
  // Error display
  // --------------------------------------------------------------------------

  describe("Error display", () => {
    it("shows settings error in dialog footer", () => {
      uiState.activeModal = "settings";
      render(<SettingsDialog {...{ ...defaultProps, settingsError: "Save failed" }} />);
      expect(screen.getByText("Save failed")).toBeInTheDocument();
    });

    it("does not show footer error when settingsError is null", () => {
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);
      expect(screen.queryByText("Save failed")).not.toBeInTheDocument();
    });
  });

  // --------------------------------------------------------------------------
  // Header
  // --------------------------------------------------------------------------

  describe("Header", () => {
    it("always shows 'Settings' label in header", () => {
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      const dialog = screen.getByTestId("settings-dialog");
      expect(within(dialog).getAllByText("Settings").length).toBeGreaterThan(0);
    });
  });
});
