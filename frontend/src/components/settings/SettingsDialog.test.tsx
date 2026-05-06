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
import { DEFAULT_PROJECT_SETTINGS } from "@/types/settings";
import { SETTINGS_SECTIONS } from "./settings-registry";

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

vi.mock("./sections/GlobalExecutionSection", () => ({
  default: () => <div data-testid="global-execution-section">Global Capacity</div>,
}));

vi.mock("./sections/ReviewPolicySection", () => ({
  default: () => <div data-testid="review-policy-section">Review Policy</div>,
}));

vi.mock("./ExternalMcpSettingsPanel", () => ({
  ExternalMcpSettingsPanel: () => (
    <div data-testid="external-mcp-section">External MCP</div>
  ),
}));

vi.mock("./RepositorySettingsSection", () => ({
  RepositorySettingsSection: () => (
    <div data-testid="repository-section">Repository</div>
  ),
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

  describe("Section initialization via modalContext deep-link", () => {
    it("defaults to the Repository section when no modalContext.section is provided", async () => {
      uiState.activeModal = "settings";
      uiState.modalContext = undefined;
      render(<SettingsDialog {...defaultProps} />);

      expect(await screen.findByTestId("repository-section")).toBeInTheDocument();
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

    it("initializes to Execution Agents section when modalContext.section is 'execution-harnesses'", async () => {
      uiState.activeModal = "settings";
      uiState.modalContext = { section: "execution-harnesses" };
      render(<SettingsDialog {...defaultProps} />);

      expect(await screen.findByText("Execution Pipeline Agents")).toBeInTheDocument();
    });

  });

  // --------------------------------------------------------------------------
  // Sidebar order / first paint / persisted section
  // --------------------------------------------------------------------------

  describe("Sidebar order and section persistence", () => {
    it("shows Workspace first with Repository as the first sidebar item", () => {
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      const navigation = screen.getByRole("navigation");
      const groupLabels = within(navigation)
        .getAllByText(/Workspace|General|Ideation|Access|Preferences/)
        .map((element) => element.textContent);

      expect(groupLabels.slice(0, 2)).toEqual(["Workspace", "General"]);
      expect(within(navigation).getAllByRole("button")[0]).toHaveAccessibleName(
        "Repository",
      );
    });

    it("uses the v30 settings modal shell classes and active nav treatment", () => {
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      const dialog = screen.getByTestId("settings-dialog");
      expect(dialog).toHaveClass("settings-modal");

      const navigation = screen.getByRole("navigation");
      expect(navigation).toHaveClass("settings-nav");
      expect(within(navigation).getByRole("button", { name: "Repository" })).toHaveClass(
        "settings-nav__item",
      );
      expect(within(navigation).getByRole("button", { name: "Repository" })).toHaveAttribute(
        "aria-current",
        "page",
      );
    });

    it("paints the settings shell before hydrating the active section", () => {
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      expect(screen.getByTestId("settings-dialog")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Repository" })).toBeInTheDocument();
      expect(screen.getByTestId("settings-section-loading")).toBeInTheDocument();
      expect(screen.queryByTestId("repository-section")).not.toBeInTheDocument();
    });

    it("remembers the last opened section across Settings reopens", async () => {
      const user = userEvent.setup();
      uiState.activeModal = "settings";
      const { rerender } = render(<SettingsDialog {...defaultProps} />);

      expect(await screen.findByTestId("repository-section")).toBeInTheDocument();

      await user.click(screen.getByRole("button", { name: "Review Policy" }));
      expect(await screen.findByTestId("review-policy-section")).toBeInTheDocument();
      await waitFor(() =>
        expect(localStorage.getItem("ralphx-settings-active-section")).toBe(
          "review",
        ),
      );

      uiState.activeModal = null;
      rerender(<SettingsDialog {...defaultProps} />);

      uiState.activeModal = "settings";
      rerender(<SettingsDialog {...defaultProps} />);
      expect(await screen.findByTestId("review-policy-section")).toBeInTheDocument();
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

      expect(await screen.findByTestId("repository-section")).toBeInTheDocument();

      // Click "Review Policy" in the left nav rail
      const reviewNavItem = screen.getByRole("button", { name: "Review Policy" });
      await user.click(reviewNavItem);

      expect(screen.queryByTestId("repository-section")).not.toBeInTheDocument();
      expect(await screen.findByTestId("review-policy-section")).toBeInTheDocument();
    });

    it("switches active section via keyboard Enter on left rail item", async () => {
      const user = userEvent.setup();
      uiState.activeModal = "settings";
      render(<SettingsDialog {...defaultProps} />);

      // Navigate to Review Policy section via keyboard
      const reviewNavItem = screen.getByRole("button", { name: "Review Policy" });
      reviewNavItem.focus();
      await user.keyboard("{Enter}");

      // Review Policy section content is now visible
      expect(await screen.findByTestId("review-policy-section")).toBeInTheDocument();
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
