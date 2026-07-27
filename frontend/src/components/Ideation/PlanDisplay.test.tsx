import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  act,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PlanDisplay, VersionedArtifactDisplay } from "./PlanDisplay";
import type { Artifact } from "@/types/artifact";

const mockGetVersionHistory = vi.fn();
const mockGetAtVersion = vi.fn();

vi.mock("@/api/artifact", () => ({
  artifactApi: {
    getAtVersion: (...args: unknown[]) => mockGetAtVersion(...args),
    getVersionHistory: (...args: unknown[]) => mockGetVersionHistory(...args),
    get: vi.fn().mockResolvedValue(null),
    getByTask: vi.fn().mockResolvedValue([]),
    getByBucket: vi.fn().mockResolvedValue([]),
  },
}));

vi.mock("./VerificationHistory", () => ({
  VerificationHistory: ({ rounds }: { rounds: { round: number; gapScore: number }[] }) => (
    <div data-testid="verification-history">
      <div>Gap Score by Round</div>
      {rounds.map((r) => (
        <div key={r.round}>R{r.round}: {r.gapScore}</div>
      ))}
    </div>
  ),
}));

const mockPlan: Artifact = {
  id: "artifact-1",
  type: "specification",
  name: "Authentication Implementation Plan",
  content: {
    type: "inline",
    text: `# Authentication Plan\n\n## Overview\nImplement JWT-based authentication system.`,
  },
  metadata: {
    createdAt: "2026-01-26T10:00:00Z",
    createdBy: "ralphx-ideation",
    version: 1,
  },
  derivedFrom: [],
  bucketId: "prd-library",
};

describe("PlanDisplay", () => {
  beforeEach(() => {
    mockGetAtVersion.mockResolvedValue(null);
    mockGetVersionHistory.mockResolvedValue([]);
  });

  it("renders plan header and starts collapsed", () => {
    render(<PlanDisplay plan={mockPlan} />);

    expect(screen.getByText("Authentication Implementation Plan")).toBeInTheDocument();
    expect(screen.queryByText("Authentication Plan")).not.toBeInTheDocument();
  });

  it("expands and renders markdown content", () => {
    render(<PlanDisplay plan={mockPlan} />);

    fireEvent.click(screen.getByRole("button", { name: /Authentication Implementation Plan/i }));

    const heading = screen.getByText("Authentication Plan");
    expect(heading).toBeInTheDocument();
    expect(heading.tagName).toBe("H1");
    expect(screen.getByText(/JWT-based authentication/i)).toBeInTheDocument();
  });

  it("shows linked proposal counts", () => {
    const { rerender } = render(<PlanDisplay plan={mockPlan} linkedProposalsCount={3} />);
    expect(screen.getByText("3 linked proposals")).toBeInTheDocument();

    rerender(<PlanDisplay plan={mockPlan} linkedProposalsCount={1} />);
    expect(screen.getByText("1 linked proposal")).toBeInTheDocument();
  });

  it("calls onEdit and onExport from action buttons", async () => {
    const user = userEvent.setup();
    const onEdit = vi.fn();
    const onExport = vi.fn();
    render(<PlanDisplay plan={mockPlan} onEdit={onEdit} onExport={onExport} />);

    // Actions are in a MoreHorizontal dropdown — open it first
    const buttons = screen.getAllByRole("button");
    const moreButton = buttons[buttons.length - 1];
    await user.click(moreButton);

    await user.click(screen.getByRole("menuitem", { name: /edit/i }));
    expect(onEdit).toHaveBeenCalledTimes(1);

    await user.click(moreButton);
    await user.click(screen.getByRole("menuitem", { name: /export/i }));
    expect(onExport).toHaveBeenCalledTimes(1);
  });

  it("shows and handles Approve action", () => {
    const onApprove = vi.fn();
    render(<PlanDisplay plan={mockPlan} showApprove={true} isApproved={false} onApprove={onApprove} />);

    fireEvent.click(screen.getByRole("button", { name: /approve/i }));
    expect(onApprove).toHaveBeenCalledTimes(1);
  });

  it("shows approved badge when already approved", () => {
    render(<PlanDisplay plan={mockPlan} showApprove={true} isApproved={true} />);

    expect(screen.getByText("Plan Approved")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /approve/i })).not.toBeInTheDocument();
  });

  it("shows and handles Verify Plan action", () => {
    const onVerifyPlan = vi.fn();
    render(
      <PlanDisplay
        plan={mockPlan}
        isApproved={true}
        onVerifyPlan={onVerifyPlan}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: /Verify Plan/i }));

    expect(onVerifyPlan).toHaveBeenCalledTimes(1);
  });

  it("shows no content for empty inline text", () => {
    const emptyPlan: Artifact = {
      ...mockPlan,
      content: { type: "inline", text: "" },
    };

    render(<PlanDisplay plan={emptyPlan} isExpanded={true} />);
    expect(screen.getByText("No content available")).toBeInTheDocument();
  });

  it("shows no content for file artifacts", () => {
    const filePlan: Artifact = {
      ...mockPlan,
      content: { type: "file", path: "/path/to/plan.md" },
    };

    render(<PlanDisplay plan={filePlan} isExpanded={true} />);
    expect(screen.getByText("No content available")).toBeInTheDocument();
  });

  describe("Create Proposals button visibility", () => {
    const onCreateProposals = vi.fn();

    it("shows Create Proposals button when verified and linkedProposalsCount is 0", () => {
      render(
        <PlanDisplay
          plan={mockPlan}
          verificationStatus="verified"
          linkedProposalsCount={0}
          onCreateProposals={onCreateProposals}
        />,
      );
      expect(screen.getByRole("button", { name: /create proposals/i })).toBeInTheDocument();
    });

    it("shows Create Proposals button when skipped and linkedProposalsCount is 0", () => {
      render(
        <PlanDisplay
          plan={mockPlan}
          verificationStatus="skipped"
          linkedProposalsCount={0}
          onCreateProposals={onCreateProposals}
        />,
      );
      expect(screen.getByRole("button", { name: /create proposals/i })).toBeInTheDocument();
    });

    it("hides Create Proposals button when verified but linkedProposalsCount > 0", () => {
      render(
        <PlanDisplay
          plan={mockPlan}
          verificationStatus="verified"
          linkedProposalsCount={2}
          onCreateProposals={onCreateProposals}
        />,
      );
      expect(screen.queryByRole("button", { name: /create proposals/i })).not.toBeInTheDocument();
    });

    it("hides Create Proposals button after proposals are created (0 → N transition)", () => {
      const { rerender } = render(
        <PlanDisplay
          plan={mockPlan}
          verificationStatus="verified"
          linkedProposalsCount={0}
          onCreateProposals={onCreateProposals}
        />,
      );
      expect(screen.getByRole("button", { name: /create proposals/i })).toBeInTheDocument();

      rerender(
        <PlanDisplay
          plan={mockPlan}
          verificationStatus="verified"
          linkedProposalsCount={3}
          onCreateProposals={onCreateProposals}
        />,
      );
      expect(screen.queryByRole("button", { name: /create proposals/i })).not.toBeInTheDocument();
    });
  });

  // ============================================================================
  // Version history timestamps
  // ============================================================================

  describe("version history timestamps", () => {
    const multiVersionPlan: Artifact = {
      ...mockPlan,
      metadata: {
        ...mockPlan.metadata,
        version: 3,
      },
    };

    it("does not show version dropdown for single-version plans", () => {
      render(<PlanDisplay plan={mockPlan} />);
      // version === 1, no History button rendered
      expect(screen.queryByTitle("View version history")).not.toBeInTheDocument();
    });

    it("calls getVersionHistory on mount for multi-version plans", async () => {
      mockGetVersionHistory.mockResolvedValue([
        { id: "v3-id", version: 3, name: "Plan", created_at: "2026-03-18T11:30:00Z" },
        { id: "v2-id", version: 2, name: "Plan", created_at: "2026-03-17T16:15:00Z" },
        { id: "v1-id", version: 1, name: "Plan", created_at: "2026-03-16T09:00:00Z" },
      ]);

      render(<PlanDisplay plan={multiVersionPlan} />);

      await waitFor(() => {
        expect(mockGetVersionHistory).toHaveBeenCalledWith(multiVersionPlan.id);
      });
    });

    it("does not call getVersionHistory for single-version plans", async () => {
      render(<PlanDisplay plan={mockPlan} />);

      // Give React a tick to run effects
      await act(async () => {});
      expect(mockGetVersionHistory).not.toHaveBeenCalled();
    });

    it("renders without error when version history fetch fails (graceful fallback)", async () => {
      mockGetVersionHistory.mockRejectedValue(new Error("Network error"));

      render(<PlanDisplay plan={multiVersionPlan} />);

      // Should not throw — component renders normally
      await waitFor(() => {
        expect(mockGetVersionHistory).toHaveBeenCalled();
      });

      // History button still visible
      expect(screen.getByTitle("View version history")).toBeInTheDocument();
    });

    it("opens dropdown and shows version numbers with userEvent", async () => {
      const user = userEvent.setup();
      mockGetVersionHistory.mockResolvedValue([
        { id: "v2-id", version: 2, name: "Plan", created_at: "2026-03-18T10:00:00Z" },
        { id: "v1-id", version: 1, name: "Plan", created_at: "2026-03-17T10:00:00Z" },
      ]);

      const twoVersionPlan: Artifact = {
        ...mockPlan,
        metadata: { ...mockPlan.metadata, version: 2 },
      };

      render(<PlanDisplay plan={twoVersionPlan} />);

      // Wait for version history to load
      await waitFor(() => {
        expect(mockGetVersionHistory).toHaveBeenCalled();
      });

      // Open dropdown
      await user.click(screen.getByTitle("View version history"));

      // Dropdown items should be visible
      await waitFor(() => {
        expect(screen.getByText("(latest)")).toBeInTheDocument();
      });
    });
  });

  // ============================================================================
  // Historical version fetch & navigation
  // ============================================================================

  describe("historical version fetch", () => {
    const multiVersionPlan: Artifact = {
      ...mockPlan,
      metadata: { ...mockPlan.metadata, version: 3 },
    };

    it("loads inline historical content when an older version is selected", async () => {
      const user = userEvent.setup();
      mockGetVersionHistory.mockResolvedValue([
        { id: "v3-id", version: 3, name: "Plan", created_at: "2026-03-18T10:00:00Z" },
        { id: "v2-id", version: 2, name: "Plan", created_at: "2026-03-17T10:00:00Z" },
        { id: "v1-id", version: 1, name: "Plan", created_at: "2026-03-16T10:00:00Z" },
      ]);
      mockGetAtVersion.mockResolvedValue({
        ...multiVersionPlan,
        content: { type: "inline", text: "# Old version\n\nHistorical body" },
      });

      render(<PlanDisplay plan={multiVersionPlan} isExpanded={true} />);

      await waitFor(() => expect(mockGetVersionHistory).toHaveBeenCalled());

      await user.click(screen.getByTitle("View version history"));
      // Click v1 entry
      const v1Items = await screen.findAllByText(/v1/);
      // Use the menuitem text node
      await user.click(v1Items[v1Items.length - 1]);

      await waitFor(() => {
        expect(mockGetAtVersion).toHaveBeenCalledWith(multiVersionPlan.id, 1);
      });
      // Banner should appear
      await waitFor(() => {
        expect(screen.getByText(/Viewing version 1 of 3/i)).toBeInTheDocument();
      });
      // Historical content rendered
      await waitFor(() => {
        expect(screen.getByText(/Historical body/i)).toBeInTheDocument();
      });

      // "Back to latest" returns to current version
      await user.click(screen.getByRole("button", { name: /back to latest/i }));
      await waitFor(() => {
        expect(screen.queryByText(/Viewing version 1 of 3/i)).not.toBeInTheDocument();
      });
    });

    it("falls back to no historical content when artifact is non-inline", async () => {
      const user = userEvent.setup();
      mockGetVersionHistory.mockResolvedValue([]);
      mockGetAtVersion.mockResolvedValue({
        ...multiVersionPlan,
        content: { type: "file", path: "/p/x.md" },
      });

      render(<PlanDisplay plan={multiVersionPlan} isExpanded={true} />);
      await waitFor(() => expect(mockGetVersionHistory).toHaveBeenCalled());

      await user.click(screen.getByTitle("View version history"));
      const items = await screen.findAllByText(/v2/);
      await user.click(items[items.length - 1]);

      await waitFor(() => {
        expect(mockGetAtVersion).toHaveBeenCalledWith(multiVersionPlan.id, 2);
      });
      // Banner shown
      await waitFor(() => {
        expect(screen.getByText(/Viewing version 2 of 3/i)).toBeInTheDocument();
      });
    });

    it("recovers gracefully when getAtVersion rejects", async () => {
      const user = userEvent.setup();
      const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
      mockGetVersionHistory.mockResolvedValue([]);
      mockGetAtVersion.mockRejectedValue(new Error("boom"));

      render(<PlanDisplay plan={multiVersionPlan} isExpanded={true} />);
      await waitFor(() => expect(mockGetVersionHistory).toHaveBeenCalled());

      await user.click(screen.getByTitle("View version history"));
      const items = await screen.findAllByText(/v1/);
      await user.click(items[items.length - 1]);

      await waitFor(() => {
        expect(errorSpy).toHaveBeenCalledWith("Failed to fetch historical version:", expect.any(Error));
      });
      errorSpy.mockRestore();
    });

    it("auto-expands when a version is selected from a collapsed state", async () => {
      const user = userEvent.setup();
      mockGetVersionHistory.mockResolvedValue([]);
      mockGetAtVersion.mockResolvedValue({
        ...multiVersionPlan,
        content: { type: "inline", text: "# old\n\nold body" },
      });

      render(<PlanDisplay plan={multiVersionPlan} />);
      await waitFor(() => expect(mockGetVersionHistory).toHaveBeenCalled());

      // Initially collapsed - markdown content not rendered
      expect(screen.queryByText("Authentication Plan")).not.toBeInTheDocument();

      await user.click(screen.getByTitle("View version history"));
      const items = await screen.findAllByText(/v1/);
      await user.click(items[items.length - 1]);

      // Auto expanded - banner visible
      await waitFor(() => {
        expect(screen.getByText(/Viewing version 1 of 3/i)).toBeInTheDocument();
      });
    });

    it("applies requestedVersion from parent and notifies via onVersionViewed", async () => {
      mockGetVersionHistory.mockResolvedValue([]);
      mockGetAtVersion.mockResolvedValue({
        ...multiVersionPlan,
        content: { type: "inline", text: "# v2\n\nv2 body" },
      });
      const onVersionViewed = vi.fn();

      render(
        <PlanDisplay
          plan={multiVersionPlan}
          isExpanded={true}
          requestedVersion={2}
          onVersionViewed={onVersionViewed}
        />,
      );

      await waitFor(() => {
        expect(mockGetAtVersion).toHaveBeenCalledWith(multiVersionPlan.id, 2);
      });
      await waitFor(() => {
        expect(onVersionViewed).toHaveBeenCalled();
      });
    });
  });

  // ============================================================================
  // Default export (Blob fallback)
  // ============================================================================

  describe("default export fallback", () => {
    it("creates a download link when onExport is not provided", async () => {
      const user = userEvent.setup();
      const createObjectURL = vi.fn().mockReturnValue("blob:http://test/abc");
      const revokeObjectURL = vi.fn();
      const origCreate = URL.createObjectURL;
      const origRevoke = URL.revokeObjectURL;
      Object.defineProperty(URL, "createObjectURL", { value: createObjectURL, configurable: true });
      Object.defineProperty(URL, "revokeObjectURL", { value: revokeObjectURL, configurable: true });
      const clickSpy = vi.fn();
      const origAnchorClick = HTMLAnchorElement.prototype.click;
      HTMLAnchorElement.prototype.click = clickSpy;

      try {
        render(<PlanDisplay plan={mockPlan} />);
        const buttons = screen.getAllByRole("button");
        const moreButton = buttons[buttons.length - 1]!;
        await user.click(moreButton);
        await user.click(screen.getByRole("menuitem", { name: /export/i }));

        expect(createObjectURL).toHaveBeenCalled();
        expect(clickSpy).toHaveBeenCalled();
        expect(revokeObjectURL).toHaveBeenCalled();
      } finally {
        HTMLAnchorElement.prototype.click = origAnchorClick;
        Object.defineProperty(URL, "createObjectURL", { value: origCreate, configurable: true });
        Object.defineProperty(URL, "revokeObjectURL", { value: origRevoke, configurable: true });
      }
    });
  });

  // ============================================================================
  // Copy Markdown action
  // ============================================================================

  describe("copy markdown", () => {
    it("copies inline plan text to clipboard from overflow menu", async () => {
      const user = userEvent.setup();
      const writeText = vi.fn().mockResolvedValue(undefined);
      Object.defineProperty(navigator, "clipboard", {
        value: { writeText },
        configurable: true,
        writable: true,
      });

      render(<PlanDisplay plan={mockPlan} />);
      const buttons = screen.getAllByRole("button");
      const moreButton = buttons[buttons.length - 1]!;
      await user.click(moreButton);

      await user.click(screen.getByRole("menuitem", { name: /copy markdown/i }));
      expect(writeText).toHaveBeenCalledWith(expect.stringContaining("Authentication Plan"));
    });

    it("disables copy menu item for file artifacts", async () => {
      const user = userEvent.setup();
      const filePlan: Artifact = {
        ...mockPlan,
        content: { type: "file", path: "/p/plan.md" },
      };

      render(<PlanDisplay plan={filePlan} />);
      const buttons = screen.getAllByRole("button");
      const moreButton = buttons[buttons.length - 1]!;
      await user.click(moreButton);

      const copyItem = screen.getByRole("menuitem", { name: /copy markdown/i });
      expect(copyItem).toHaveAttribute("aria-disabled", "true");
    });
  });

  // ============================================================================
  // New conversation action
  // ============================================================================

  describe("new conversation action", () => {
    it("only shows the action when a callback is provided", async () => {
      const user = userEvent.setup();

      render(<PlanDisplay plan={mockPlan} />);
      await user.click(screen.getByLabelText("Plan actions"));

      expect(
        screen.queryByRole("menuitem", { name: /new conversation/i }),
      ).not.toBeInTheDocument();
    });

    it("hides the action for a selected historical version", async () => {
      const user = userEvent.setup();
      const onStartNewConversationWithPlan = vi.fn();
      const multiVersionPlan: Artifact = {
        ...mockPlan,
        metadata: { ...mockPlan.metadata, version: 3 },
      };
      mockGetVersionHistory.mockResolvedValue([
        { id: "v3-id", version: 3, name: "Plan", created_at: "2026-03-18T10:00:00Z" },
        { id: "v2-id", version: 2, name: "Plan", created_at: "2026-03-17T10:00:00Z" },
        { id: "v1-id", version: 1, name: "Plan", created_at: "2026-03-16T10:00:00Z" },
      ]);
      mockGetAtVersion.mockResolvedValue({
        ...multiVersionPlan,
        content: { type: "inline", text: "# Old version\n\nHistorical body" },
      });

      render(
        <PlanDisplay
          plan={multiVersionPlan}
          onStartNewConversationWithPlan={onStartNewConversationWithPlan}
          disableHistoricalNewConversation
        />,
      );

      await waitFor(() => expect(mockGetVersionHistory).toHaveBeenCalled());
      await user.click(screen.getByTitle("View version history"));
      const v1Items = await screen.findAllByText(/v1/);
      await user.click(v1Items[v1Items.length - 1]!);
      await waitFor(() => {
        expect(mockGetAtVersion).toHaveBeenCalledWith(multiVersionPlan.id, 1);
      });

      await user.click(screen.getByLabelText("Plan actions"));
      expect(
        screen.queryByRole("menuitem", { name: /new conversation/i }),
      ).not.toBeInTheDocument();
      expect(onStartNewConversationWithPlan).not.toHaveBeenCalled();
    });
  });

  // ============================================================================
  // Hover handlers (mouse enter/leave) — chrome buttons & history dropdown
  // ============================================================================

  describe("hover handlers", () => {
    it("invokes mouse enter/leave handlers for Approve in wrapper card without throwing", () => {
      render(<PlanDisplay plan={mockPlan} showApprove={true} onApprove={() => {}} />);
      const approveBtn = screen.getByRole("button", { name: /approve/i });
      fireEvent.mouseEnter(approveBtn);
      fireEvent.mouseLeave(approveBtn);
      expect(approveBtn).toBeInTheDocument();
    });

    it("invokes mouse enter/leave handlers on Create Proposals button", () => {
      render(
        <PlanDisplay
          plan={mockPlan}
          linkedProposalsCount={0}
          onCreateProposals={() => {}}
        />,
      );
      const btn = screen.getByRole("button", { name: /create proposals/i });
      fireEvent.mouseEnter(btn);
      fireEvent.mouseLeave(btn);
      expect(btn).toBeInTheDocument();
    });

    it("invokes hover handlers on history dropdown trigger and overflow trigger", async () => {
      mockGetVersionHistory.mockResolvedValue([]);
      const multi: Artifact = { ...mockPlan, metadata: { ...mockPlan.metadata, version: 2 } };
      render(<PlanDisplay plan={multi} />);

      const historyBtn = screen.getByTitle("View version history");
      fireEvent.mouseEnter(historyBtn);
      fireEvent.mouseLeave(historyBtn);

      // overflow ("..." MoreHorizontal) is the last button
      const buttons = screen.getAllByRole("button");
      const moreBtn = buttons[buttons.length - 1]!;
      fireEvent.mouseEnter(moreBtn);
      fireEvent.mouseLeave(moreBtn);

      expect(historyBtn).toBeInTheDocument();
    });

    it("invokes hover handlers on Back to latest button", async () => {
      const user = userEvent.setup();
      const multi: Artifact = { ...mockPlan, metadata: { ...mockPlan.metadata, version: 2 } };
      mockGetVersionHistory.mockResolvedValue([]);
      mockGetAtVersion.mockResolvedValue({
        ...multi,
        content: { type: "inline", text: "# old\n\nold body" },
      });

      render(<PlanDisplay plan={multi} isExpanded={true} />);
      await waitFor(() => expect(mockGetVersionHistory).toHaveBeenCalled());

      await user.click(screen.getByTitle("View version history"));
      const items = await screen.findAllByText(/v1/);
      await user.click(items[items.length - 1]);

      const back = await screen.findByRole("button", { name: /back to latest/i });
      fireEvent.mouseEnter(back);
      fireEvent.mouseLeave(back);
      expect(back).toBeInTheDocument();
    });
  });

  // ============================================================================
  // Chromeless rendering
  // ============================================================================

  describe("chromeless mode", () => {
    it("uses extracted H1 as title and skips wrapper chrome", () => {
      render(<PlanDisplay plan={mockPlan} chromeless={true} />);
      // Body H1 should be hoisted as the chromeless title
      expect(screen.getByTestId("plan-display-chromeless")).toBeInTheDocument();
      expect(screen.getByText("Authentication Plan")).toBeInTheDocument();
    });

    it("falls back to plan.name as title when content has no leading H1", () => {
      const noH1: Artifact = {
        ...mockPlan,
        content: { type: "inline", text: "Just a body, no heading" },
      };
      render(<PlanDisplay plan={noH1} chromeless={true} />);
      expect(screen.getByText("Authentication Implementation Plan")).toBeInTheDocument();
      expect(screen.getByText(/Just a body/i)).toBeInTheDocument();
    });

    it("renders 'No content available' for empty inline body in chromeless mode", () => {
      const empty: Artifact = { ...mockPlan, content: { type: "inline", text: "" } };
      render(<PlanDisplay plan={empty} chromeless={true} />);
      expect(screen.getByText("No content available")).toBeInTheDocument();
    });

    it("renders Approve in chromeless mode and dispatches onApprove", () => {
      const onApprove = vi.fn();
      render(
        <PlanDisplay plan={mockPlan} chromeless={true} showApprove={true} onApprove={onApprove} />,
      );
      const btn = screen.getByRole("button", { name: /approve/i });
      fireEvent.mouseEnter(btn);
      fireEvent.mouseLeave(btn);
      fireEvent.click(btn);
      expect(onApprove).toHaveBeenCalledTimes(1);
    });

    it("renders Approved badge in chromeless mode when isApproved", () => {
      render(
        <PlanDisplay plan={mockPlan} chromeless={true} showApprove={true} isApproved={true} />,
      );
      expect(screen.getByText("Plan Approved")).toBeInTheDocument();
    });

    it("supports the same document surface for a non-plan artifact without false Plan selection", () => {
      render(
        <VersionedArtifactDisplay
          artifact={{ ...mockPlan, type: "persona", name: "Support Voice" }}
          artifactLabel="Persona"
          chromeless={true}
          excerptSelectionEnabled={false}
          artifactActions={<button type="button">Refine Persona</button>}
        />,
      );

      expect(screen.getByTestId("plan-display-chromeless")).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "Refine Persona" }),
      ).toBeInTheDocument();
      expect(document.querySelector("[data-artifact-selectable-region]")).toBeNull();
    });

    it("renders Overview and Blueprint tabs plus conditional Proposals after approval", () => {
      const onBodyModeChange = vi.fn();
      render(
        <PlanDisplay
          plan={mockPlan}
          chromeless={true}
          isApproved={true}
          linkedProposalsCount={2}
          bodyMode="overview"
          onBodyModeChange={onBodyModeChange}
        />,
      );

      const approvedBadge = screen.getByText("Plan Approved");
      const overviewButton = screen.getByRole("tab", {
        name: /Overview/i,
      });
      const blueprintButton = screen.getByRole("tab", {
        name: /Blueprint/i,
      });
      const proposalsButton = screen.getByRole("tab", {
        name: /Proposals \(2\)/i,
      });
      expect(approvedBadge.compareDocumentPosition(overviewButton)).toBe(
        Node.DOCUMENT_POSITION_FOLLOWING,
      );
      expect(overviewButton).toHaveAttribute("aria-selected", "true");
      expect(blueprintButton).toHaveAttribute("aria-selected", "false");
      expect(proposalsButton).toHaveAttribute("aria-selected", "false");
      const overviewPanel = document.getElementById(
        overviewButton.getAttribute("aria-controls")!,
      );
      expect(overviewPanel).toHaveAttribute("role", "tabpanel");
      expect(overviewPanel).toBeVisible();
      expect(
        within(overviewPanel!).getByText(
          "Implement JWT-based authentication system.",
        ),
      ).toBeVisible();

      fireEvent.mouseDown(proposalsButton);
      fireEvent.click(proposalsButton);

      expect(onBodyModeChange).toHaveBeenCalledWith("proposals");
    });

    it("renders Create Proposals in chromeless mode and dispatches handler", () => {
      const onCreateProposals = vi.fn();
      render(
        <PlanDisplay
          plan={mockPlan}
          chromeless={true}
          linkedProposalsCount={0}
          onCreateProposals={onCreateProposals}
        />,
      );
      const btn = screen.getByRole("button", { name: /create proposals/i });
      fireEvent.mouseEnter(btn);
      fireEvent.mouseLeave(btn);
      fireEvent.click(btn);
      expect(onCreateProposals).toHaveBeenCalledTimes(1);
    });

    it("renders Verify Plan on the chromeless action row and dispatches handler", () => {
      const onVerifyPlan = vi.fn();
      render(
        <PlanDisplay
          plan={mockPlan}
          chromeless={true}
          onVerifyPlan={onVerifyPlan}
        />,
      );

      const btn = screen.getByRole("button", { name: /verify plan/i });
      fireEvent.mouseEnter(btn);
      fireEvent.mouseLeave(btn);
      fireEvent.click(btn);
      expect(onVerifyPlan).toHaveBeenCalledTimes(1);
    });

    it("renders Create Proposals before Implement Directly when proposals are primary", () => {
      const onCreateProposals = vi.fn();
      const onImplementDirectly = vi.fn();
      render(
        <PlanDisplay
          plan={mockPlan}
          chromeless={true}
          linkedProposalsCount={0}
          onCreateProposals={onCreateProposals}
          onImplementDirectly={onImplementDirectly}
          primaryPlanAction="create_proposals"
        />,
      );

      const createButton = screen.getByRole("button", { name: /create proposals/i });
      const implementButton = screen.getByRole("button", { name: /implement directly/i });
      expect(createButton.compareDocumentPosition(implementButton) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

      fireEvent.mouseEnter(createButton);
      fireEvent.mouseLeave(createButton);
      fireEvent.click(createButton);
      expect(onCreateProposals).toHaveBeenCalledTimes(1);

      fireEvent.mouseEnter(implementButton);
      fireEvent.mouseLeave(implementButton);
      fireEvent.click(implementButton);
      expect(onImplementDirectly).toHaveBeenCalledTimes(1);
    });

    it("renders Implement Directly before Create Proposals when implementation is primary", () => {
      const onCreateProposals = vi.fn();
      const onImplementDirectly = vi.fn();
      render(
        <PlanDisplay
          plan={mockPlan}
          chromeless={true}
          linkedProposalsCount={0}
          onCreateProposals={onCreateProposals}
          onImplementDirectly={onImplementDirectly}
          primaryPlanAction="implement_directly"
        />,
      );

      const implementButton = screen.getByRole("button", { name: /implement directly/i });
      const createButton = screen.getByRole("button", { name: /create proposals/i });
      expect(implementButton.compareDocumentPosition(createButton) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

      fireEvent.mouseEnter(implementButton);
      fireEvent.mouseLeave(implementButton);
      fireEvent.click(implementButton);
      expect(onImplementDirectly).toHaveBeenCalledTimes(1);

      fireEvent.mouseEnter(createButton);
      fireEvent.mouseLeave(createButton);
      fireEvent.click(createButton);
      expect(onCreateProposals).toHaveBeenCalledTimes(1);
    });

    it("hides overflow + version-history when showOverflowActions=false", () => {
      const multi: Artifact = { ...mockPlan, metadata: { ...mockPlan.metadata, version: 2 } };
      render(
        <PlanDisplay plan={multi} chromeless={true} showOverflowActions={false} />,
      );
      expect(screen.queryByTitle("View version history")).not.toBeInTheDocument();
      expect(screen.queryByLabelText("Plan actions")).not.toBeInTheDocument();
    });

    it("shows version dropdown in chromeless mode for multi-version plans", async () => {
      const user = userEvent.setup();
      mockGetVersionHistory.mockResolvedValue([
        { id: "v2-id", version: 2, name: "Plan", created_at: "2026-03-18T10:00:00Z" },
        { id: "v1-id", version: 1, name: "Plan", created_at: "2026-03-17T10:00:00Z" },
      ]);
      const multi: Artifact = { ...mockPlan, metadata: { ...mockPlan.metadata, version: 2 } };

      render(<PlanDisplay plan={multi} chromeless={true} />);
      await waitFor(() => expect(mockGetVersionHistory).toHaveBeenCalled());

      await user.click(screen.getByTitle("View version history"));
      await waitFor(() => {
        expect(screen.getByText("(latest)")).toBeInTheDocument();
      });
    });

    it("renders historical banner + Back to latest in chromeless mode", async () => {
      const user = userEvent.setup();
      const multi: Artifact = { ...mockPlan, metadata: { ...mockPlan.metadata, version: 2 } };
      mockGetVersionHistory.mockResolvedValue([]);
      mockGetAtVersion.mockResolvedValue({
        ...multi,
        content: { type: "inline", text: "# old\n\nold chrome body" },
      });

      render(<PlanDisplay plan={multi} chromeless={true} />);
      await waitFor(() => expect(mockGetVersionHistory).toHaveBeenCalled());

      await user.click(screen.getByTitle("View version history"));
      const items = await screen.findAllByText(/v1/);
      await user.click(items[items.length - 1]);

      const back = await screen.findByRole("button", { name: /back to latest/i });
      await user.click(back);
      await waitFor(() => {
        expect(screen.queryByText(/Viewing version 1 of 2/i)).not.toBeInTheDocument();
      });
    });

    it("calls onEdit and default export from chromeless overflow menu", async () => {
      const user = userEvent.setup();
      const onEdit = vi.fn();
      const onExport = vi.fn();
      render(
        <PlanDisplay plan={mockPlan} chromeless={true} onEdit={onEdit} onExport={onExport} />,
      );

      const more = screen.getByLabelText("Plan actions");
      await user.click(more);
      await user.click(screen.getByRole("menuitem", { name: /edit/i }));
      expect(onEdit).toHaveBeenCalled();

      await user.click(more);
      await user.click(screen.getByRole("menuitem", { name: /export/i }));
      expect(onExport).toHaveBeenCalled();
    });

    it("copies inline content from chromeless overflow menu", async () => {
      const user = userEvent.setup();
      const writeText = vi.fn().mockResolvedValue(undefined);
      Object.defineProperty(navigator, "clipboard", {
        value: { writeText },
        configurable: true,
        writable: true,
      });

      render(<PlanDisplay plan={mockPlan} chromeless={true} />);
      await user.click(screen.getByLabelText("Plan actions"));
      await user.click(screen.getByRole("menuitem", { name: /copy markdown/i }));
      expect(writeText).toHaveBeenCalled();
    });

    it("calls new conversation action from chromeless overflow menu", async () => {
      const user = userEvent.setup();
      const onStartNewConversationWithPlan = vi.fn();

      render(
        <PlanDisplay
          plan={mockPlan}
          chromeless={true}
          onStartNewConversationWithPlan={onStartNewConversationWithPlan}
        />,
      );
      await user.click(screen.getByLabelText("Plan actions"));
      await user.click(screen.getByRole("menuitem", { name: /new conversation/i }));

      expect(onStartNewConversationWithPlan).toHaveBeenCalledWith({
        artifactId: mockPlan.id,
        title: mockPlan.name,
        version: mockPlan.metadata.version,
      });
    });

    it("handles clipboard copy failure from the chromeless overflow menu", async () => {
      const user = userEvent.setup();
      const writeText = vi.fn().mockRejectedValue(new Error("denied"));
      Object.defineProperty(navigator, "clipboard", {
        value: { writeText },
        configurable: true,
        writable: true,
      });

      render(<PlanDisplay plan={mockPlan} chromeless={true} />);
      await user.click(screen.getByLabelText("Plan actions"));
      await user.click(screen.getByRole("menuitem", { name: /copy markdown/i }));
      await waitFor(() => {
        expect(writeText).toHaveBeenCalled();
      });
    });
  });

  // ============================================================================
  // Controlled expansion + plan id reset
  // ============================================================================

  describe("controlled expansion & plan changes", () => {
    it("calls onExpandedChange when toggling header in controlled mode", () => {
      const onExpandedChange = vi.fn();
      render(
        <PlanDisplay
          plan={mockPlan}
          isExpanded={false}
          onExpandedChange={onExpandedChange}
        />,
      );
      fireEvent.click(screen.getByRole("button", { name: /Authentication Implementation Plan/i }));
      expect(onExpandedChange).toHaveBeenCalledWith(true);
    });

    it("resets selected version when plan id changes", async () => {
      const v3: Artifact = { ...mockPlan, metadata: { ...mockPlan.metadata, version: 3 } };
      mockGetVersionHistory.mockResolvedValue([]);

      const { rerender } = render(<PlanDisplay plan={v3} />);
      await waitFor(() => expect(mockGetVersionHistory).toHaveBeenCalledWith(v3.id));

      const v3New: Artifact = {
        ...v3,
        id: "artifact-2",
        metadata: { ...v3.metadata, version: 3 },
      };
      rerender(<PlanDisplay plan={v3New} />);
      await waitFor(() => expect(mockGetVersionHistory).toHaveBeenCalledWith("artifact-2"));
    });
  });
});
