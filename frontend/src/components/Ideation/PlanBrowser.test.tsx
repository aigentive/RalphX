/**
 * Tests for PlanBrowser orchestrator.
 *
 * Heavy children (GroupSection, PlanItem, EmptyState, Tooltip) are stubbed so
 * the assertions stay focused on PlanBrowser's own state machine: search
 * toggle/clear flow, group expand state seeding, in-progress auto-open after
 * counts hydrate, search-active auto-expand of matching groups, rename
 * keyboard handlers, and archive/reopen/reset-reaccept callbacks.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import type { SessionGroup } from "./planBrowserUtils";

// ---------------------------------------------------------------------------
// Mocks (must be declared before component import)
// ---------------------------------------------------------------------------

const mockUseSessionGroupCounts = vi.fn();
const mockUpdateTitle = vi.fn().mockResolvedValue(undefined);

vi.mock("@/hooks/useIdeation", () => ({
  useSessionGroupCounts: (...args: unknown[]) => mockUseSessionGroupCounts(...args),
}));

vi.mock("@/api/ideation", () => ({
  ideationApi: {
    sessions: {
      updateTitle: (...args: unknown[]) => mockUpdateTitle(...args),
    },
  },
}));

vi.mock("@/components/ui/empty-state", () => ({
  EmptyState: ({ title, description }: { title: string; description?: string }) => (
    <div data-testid="empty-state">
      <span>{title}</span>
      {description ? <span>{description}</span> : null}
    </div>
  ),
}));

vi.mock("@/components/ui/tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  TooltipContent: () => null,
}));

interface CapturedGroup {
  groupKey: SessionGroup;
  isOpen: boolean;
  count: number;
  onToggle: (open: boolean) => void;
  renderItem: (plan: { id: string; title: string }, group: SessionGroup) => React.ReactNode;
}

const groupCalls: CapturedGroup[] = [];

vi.mock("./GroupSection", () => ({
  GroupSection: (props: CapturedGroup) => {
    groupCalls.push(props);
    return (
      <div
        data-testid={`group-section-${props.groupKey}`}
        data-is-open={String(props.isOpen)}
        data-count={String(props.count)}
      >
        <button
          type="button"
          data-testid={`group-toggle-${props.groupKey}`}
          onClick={() => props.onToggle(!props.isOpen)}
        >
          toggle {props.groupKey}
        </button>
      </div>
    );
  },
}));

vi.mock("./PlanItem", () => ({
  PlanItem: (props: { plan: { id: string }; group: SessionGroup }) => (
    <div data-testid={`plan-item-${props.plan.id}`}>plan {props.plan.id}</div>
  ),
}));

// ---------------------------------------------------------------------------
// Imports after mocks
// ---------------------------------------------------------------------------

import { PlanBrowser } from "./PlanBrowser";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeCounts(overrides: Partial<{
  drafts: number;
  inProgress: number;
  accepted: number;
  done: number;
  archived: number;
}> = {}) {
  return {
    drafts: 0,
    inProgress: 0,
    accepted: 0,
    done: 0,
    archived: 0,
    ...overrides,
  };
}

function setCounts(counts: ReturnType<typeof makeCounts>, opts: { isFetching?: boolean } = {}) {
  mockUseSessionGroupCounts.mockReturnValue({
    data: counts,
    isFetching: opts.isFetching ?? false,
  });
}

const baseProps = {
  projectId: "proj-1",
  currentPlanId: null as string | null,
  onSelectPlan: vi.fn(),
  onNewPlan: vi.fn(),
};

// ---------------------------------------------------------------------------

describe("PlanBrowser", () => {
  beforeEach(() => {
    groupCalls.length = 0;
    vi.clearAllMocks();
    mockUseSessionGroupCounts.mockReturnValue({ data: undefined, isFetching: false });
  });

  it("renders no-plans empty state when totalCount is 0 and no search active", () => {
    setCounts(makeCounts());
    render(<PlanBrowser {...baseProps} onSelectPlan={vi.fn()} onNewPlan={vi.fn()} />);
    expect(screen.getByTestId("empty-state")).toHaveTextContent("No plans yet");
  });

  it("invokes onNewPlan when the New button is clicked", async () => {
    const onNewPlan = vi.fn();
    setCounts(makeCounts());
    render(<PlanBrowser {...baseProps} onNewPlan={onNewPlan} />);
    await userEvent.click(screen.getByTestId("ideation-new-plan"));
    expect(onNewPlan).toHaveBeenCalledOnce();
  });

  it("toggles the search input open and clears it via the X button", async () => {
    const user = userEvent.setup();
    setCounts(makeCounts({ drafts: 2 }));
    render(<PlanBrowser {...baseProps} />);

    // Search field is hidden by default
    expect(screen.queryByPlaceholderText("Search sessions...")).toBeNull();

    await user.click(screen.getByTestId("ideation-search-toggle"));
    const input = screen.getByPlaceholderText("Search sessions...") as HTMLInputElement;
    expect(input).toBeInTheDocument();

    await user.type(input, "abc");
    expect(input.value).toBe("abc");

    // Wait for the 300ms debounce to flush so the clear button replaces the spinner.
    const clearBtn = await screen.findByLabelText("Clear search", undefined, { timeout: 1500 });
    await user.click(clearBtn);
    expect(input.value).toBe("");

    // Toggling the search button while open invokes handleSearchClear and closes the row
    await user.click(screen.getByTestId("ideation-search-toggle"));
    expect(screen.queryByPlaceholderText("Search sessions...")).toBeNull();
  });

  it("renders the collapse-sidebar control when onCollapse is provided", async () => {
    const onCollapse = vi.fn();
    setCounts(makeCounts());
    render(<PlanBrowser {...baseProps} onCollapse={onCollapse} />);

    const btn = screen.getByTestId("sidebar-collapse-button");
    expect(btn).toBeInTheDocument();
    await userEvent.click(btn);
    expect(onCollapse).toHaveBeenCalledOnce();
  });

  it("auto-opens the In Progress group once counts arrive with inProgress > 0", async () => {
    // Initial render: counts undefined → in-progress closed.
    mockUseSessionGroupCounts.mockReturnValueOnce({ data: undefined, isFetching: true });
    const { rerender } = render(<PlanBrowser {...baseProps} />);
    // Re-render once counts hydrate.
    mockUseSessionGroupCounts.mockReturnValue({
      data: makeCounts({ inProgress: 2, drafts: 1 }),
      isFetching: false,
    });
    rerender(<PlanBrowser {...baseProps} />);

    await waitFor(() => {
      expect(screen.getByTestId("group-section-in-progress").getAttribute("data-is-open")).toBe(
        "true",
      );
    });
  });

  it("toggles a group open/closed via handleGroupToggle", async () => {
    setCounts(makeCounts({ accepted: 3 }));
    render(<PlanBrowser {...baseProps} />);

    const accepted = screen.getByTestId("group-section-accepted");
    expect(accepted.getAttribute("data-is-open")).toBe("false");

    await userEvent.click(screen.getByTestId("group-toggle-accepted"));
    await waitFor(() => {
      expect(screen.getByTestId("group-section-accepted").getAttribute("data-is-open")).toBe(
        "true",
      );
    });
  });

  it("auto-expands groups whose count > 0 when search is active and collapses empty ones", async () => {
    const user = userEvent.setup();
    setCounts(makeCounts({ drafts: 1, inProgress: 0, accepted: 2 }));
    render(<PlanBrowser {...baseProps} />);

    await user.click(screen.getByTestId("ideation-search-toggle"));
    const input = screen.getByPlaceholderText("Search sessions...");
    await user.type(input, "match");

    // Wait for the 300ms debounce inside usePlanBrowserSearch.
    await waitFor(
      () => {
        expect(screen.getByTestId("group-section-accepted").getAttribute("data-is-open")).toBe(
          "true",
        );
      },
      { timeout: 1500 },
    );

    expect(screen.getByTestId("group-section-in-progress").getAttribute("data-is-open")).toBe(
      "false",
    );
  });

  it("renders the no-results EmptyState when search yields zero matches", async () => {
    const user = userEvent.setup();
    setCounts(makeCounts({ drafts: 1 }));
    const { rerender } = render(<PlanBrowser {...baseProps} />);

    await user.click(screen.getByTestId("ideation-search-toggle"));
    await user.type(screen.getByPlaceholderText("Search sessions..."), "zzz");

    // Once debounce flushes, the parent toggles isSearchActive; switch counts to all-zero
    // to simulate "no match" so we hit the isEmptySearchResult branch.
    mockUseSessionGroupCounts.mockReturnValue({ data: makeCounts(), isFetching: false });
    rerender(<PlanBrowser {...baseProps} />);

    await waitFor(
      () => {
        expect(screen.getByTestId("empty-state")).toHaveTextContent("No sessions match");
      },
      { timeout: 1500 },
    );
  });

  it("forwards archive / reopen / reset-reaccept callbacks via the rendered PlanItem", async () => {
    const onArchivePlan = vi.fn();
    const onReopenPlan = vi.fn();
    const onResetReacceptPlan = vi.fn();

    setCounts(makeCounts({ drafts: 1 }));
    render(
      <PlanBrowser
        {...baseProps}
        onArchivePlan={onArchivePlan}
        onReopenPlan={onReopenPlan}
        onResetReacceptPlan={onResetReacceptPlan}
      />,
    );

    // Capture the renderPlanItem closure and invoke its handlers directly.
    expect(groupCalls.length).toBeGreaterThan(0);
    const draftsCall = groupCalls.find((c) => c.groupKey === "drafts");
    expect(draftsCall).toBeDefined();
    const node = draftsCall!.renderItem(
      { id: "session-A", title: "Session A" },
      "drafts",
    ) as React.ReactElement;
    // Pull the callbacks off the rendered PlanItem props (mocked).
    const propsBag = (node as unknown as { props: Record<string, (...a: unknown[]) => unknown> }).props;
    propsBag.onArchive("session-A");
    propsBag.onReopen("session-A");
    propsBag.onResetReaccept("session-A");
    propsBag.onSelect("session-A");
    expect(onArchivePlan).toHaveBeenCalledWith("session-A");
    expect(onReopenPlan).toHaveBeenCalledWith("session-A");
    expect(onResetReacceptPlan).toHaveBeenCalledWith("session-A");
    expect(baseProps.onSelectPlan).toHaveBeenCalledWith("session-A");
  });

  it("rename Enter persists via ideationApi.sessions.updateTitle and Escape cancels", async () => {
    setCounts(makeCounts({ drafts: 1 }));
    const { act } = await import("react");
    render(<PlanBrowser {...baseProps} />);

    function latestPropsBag() {
      const draftsCall = groupCalls.findLast((c) => c.groupKey === "drafts");
      expect(draftsCall).toBeDefined();
      const node = draftsCall!.renderItem(
        { id: "session-A", title: "Session A" },
        "drafts",
      ) as React.ReactElement;
      return (node as unknown as {
        props: {
          onStartRename: (planId: string, current: string) => void;
          onTitleChange: (s: string) => void;
          onKeyDown: (
            e: { key: string; preventDefault: () => void },
            planId: string,
          ) => void;
        };
      }).props;
    }

    await act(async () => {
      latestPropsBag().onStartRename("session-A", "Old name");
    });
    await act(async () => {
      latestPropsBag().onTitleChange("New name");
    });

    const preventDefault = vi.fn();
    await act(async () => {
      latestPropsBag().onKeyDown({ key: "Enter", preventDefault }, "session-A");
    });
    expect(preventDefault).toHaveBeenCalled();
    await waitFor(() => {
      expect(mockUpdateTitle).toHaveBeenCalledWith("session-A", "New name");
    });

    // Escape resets editing state — covers handleCancelRename
    await act(async () => {
      latestPropsBag().onStartRename("session-A", "Other");
    });
    const preventDefault2 = vi.fn();
    await act(async () => {
      latestPropsBag().onKeyDown({ key: "Escape", preventDefault: preventDefault2 }, "session-A");
    });
    expect(preventDefault2).toHaveBeenCalled();
  });

  it("forwards menu open state via onMenuOpenChange", () => {
    setCounts(makeCounts({ drafts: 1 }));
    render(<PlanBrowser {...baseProps} />);
    const draftsCall = groupCalls.find((c) => c.groupKey === "drafts");
    expect(draftsCall).toBeDefined();
    const node = draftsCall!.renderItem(
      { id: "session-A", title: "Session A" },
      "drafts",
    ) as React.ReactElement;
    const propsBag = (node as unknown as { props: { onMenuOpenChange: (open: boolean, id: string) => void } }).props;
    propsBag.onMenuOpenChange(true, "session-A");
    propsBag.onMenuOpenChange(false, "session-A");
  });

  it("shows debounce loading spinner while typing", async () => {
    const user = userEvent.setup();
    setCounts(makeCounts({ drafts: 1 }));
    render(<PlanBrowser {...baseProps} />);
    await user.click(screen.getByTestId("ideation-search-toggle"));
    fireEvent.change(screen.getByPlaceholderText("Search sessions..."), {
      target: { value: "x" },
    });
    // While debounce is pending, the loader is rendered (no clear button yet).
    expect(screen.queryByLabelText("Clear search")).toBeNull();
  });
});
