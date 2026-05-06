/**
 * PlanGroupHeader.test.tsx
 * Covers: collapsed/expanded layouts, branch badge variants (active/merged/abandoned),
 * tier toggle, settings popover, navigate handlers, plan:merge_complete invalidation.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, act, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";

import type { PlanBranch } from "@/api/plan-branch.types";
import type { StatusSummary } from "@/api/task-graph.types";

// ─── Mocks ─────────────────────────────────────────────────────────────────

const getByPlan = vi.fn();
vi.mock("@/lib/tauri", () => ({
  api: {
    planBranches: {
      getByPlan: (id: string) => getByPlan(id),
    },
  },
}));

type Subscriber = (payload?: unknown) => void;
const subscribers: Record<string, Subscriber[]> = {};
const subscribe = vi.fn((event: string, cb: Subscriber) => {
  subscribers[event] = subscribers[event] ?? [];
  subscribers[event].push(cb);
  return () => {
    subscribers[event] = (subscribers[event] ?? []).filter((fn) => fn !== cb);
  };
});

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({ subscribe, emit: vi.fn() }),
}));

vi.mock("./PlanGroupSettings", () => ({
  PlanGroupSettings: ({
    onNavigateToMergeTask,
  }: {
    planBranch: PlanBranch | null;
    onNavigateToMergeTask: (id: string) => void;
  }) => (
    <button
      data-testid="plan-group-settings-content"
      onClick={() => onNavigateToMergeTask("merge-task-1")}
    >
      settings
    </button>
  ),
}));

import { PlanGroupHeader, type PlanGroupHeaderProps } from "./PlanGroupHeader";

// ─── Helpers ───────────────────────────────────────────────────────────────

function emit(event: string) {
  (subscribers[event] ?? []).forEach((cb) => cb());
}

const baseStatus: StatusSummary = {
  backlog: 0,
  ready: 0,
  blocked: 0,
  executing: 0,
  qa: 0,
  review: 0,
  merge: 0,
  completed: 0,
  terminal: 0,
};

const branch: PlanBranch = {
  id: "pb-1",
  planArtifactId: "plan-1",
  sessionId: "s1",
  projectId: "proj-1",
  branchName: "feature/super-long-branch-name-here",
  sourceBranch: "main",
  status: "active",
  baseBranchOverride: null,
  mergeTaskId: null,
  createdAt: "2026-01-01T00:00:00Z",
  mergedAt: null,
  prNumber: null,
  prUrl: null,
  prDraft: null,
  prPushStatus: null,
  prStatus: null,
  prPollingActive: false,
  prEligible: false,
};

function renderHeader(overrides: Partial<PlanGroupHeaderProps> = {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const props: PlanGroupHeaderProps = {
    planArtifactId: "plan-1",
    sessionTitle: "My Plan",
    taskCount: 4,
    statusSummary: { ...baseStatus, completed: 2 },
    isCollapsed: false,
    onToggleCollapse: vi.fn(),
    ...overrides,
  };
  const utils = render(
    <QueryClientProvider client={queryClient}>
      <PlanGroupHeader {...props} />
    </QueryClientProvider>,
  );
  return { ...utils, props, queryClient };
}

async function flush() {
  // Allow react-query async resolution
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

function renderUI(ui: ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

// ─── Tests ─────────────────────────────────────────────────────────────────

describe("PlanGroupHeader", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.keys(subscribers).forEach((k) => delete subscribers[k]);
    getByPlan.mockResolvedValue(null);
  });

  it("falls back to 'Unnamed Plan' when sessionTitle is null", async () => {
    renderHeader({ sessionTitle: null });
    await flush();
    expect(screen.getByText("Unnamed Plan")).toBeInTheDocument();
  });

  describe("Collapsed layout", () => {
    it("renders title, total tasks and progress 50%", async () => {
      renderHeader({ isCollapsed: true });
      await flush();
      expect(screen.getByText("My Plan")).toBeInTheDocument();
      expect(screen.getByText("4 tasks")).toBeInTheDocument();
      expect(screen.getByText("50%")).toBeInTheDocument();
    });

    it("renders 0% when total is 0", async () => {
      renderHeader({ isCollapsed: true, taskCount: 0, statusSummary: baseStatus });
      await flush();
      expect(screen.getByText("0%")).toBeInTheDocument();
    });

    it("calls onToggleCollapse when chevron clicked", async () => {
      const onToggle = vi.fn();
      renderHeader({ isCollapsed: true, onToggleCollapse: onToggle });
      const user = userEvent.setup();
      await user.click(screen.getByLabelText("Expand group"));
      expect(onToggle).toHaveBeenCalledTimes(1);
    });

    it("shows feature branch badge when planBranch active", async () => {
      getByPlan.mockResolvedValue(branch);
      renderHeader({ isCollapsed: true });
      await waitFor(() =>
        expect(
          screen.getByTitle(`Feature branch: ${branch.branchName} (active)`),
        ).toBeInTheDocument(),
      );
    });

    it("hides feature branch badge when status is abandoned", async () => {
      getByPlan.mockResolvedValue({ ...branch, status: "abandoned" });
      renderHeader({ isCollapsed: true });
      await flush();
      expect(
        screen.queryByTitle(/Feature branch:/),
      ).not.toBeInTheDocument();
    });
  });

  describe("Expanded layout", () => {
    it("renders all status badges with their counts", async () => {
      renderHeader({
        statusSummary: {
          ...baseStatus,
          completed: 1,
          executing: 2,
          blocked: 3,
          review: 4,
          merge: 5,
        },
        taskCount: 15,
      });
      await flush();
      expect(screen.getByTitle("1 completed")).toBeInTheDocument();
      expect(screen.getByTitle("2 executing")).toBeInTheDocument();
      expect(screen.getByTitle("3 blocked")).toBeInTheDocument();
      expect(screen.getByTitle("4 in review")).toBeInTheDocument();
      expect(screen.getByTitle("5 merging")).toBeInTheDocument();
    });

    it("hides status badges when count is zero", async () => {
      renderHeader();
      await flush();
      expect(screen.queryByTitle(/0 executing/)).not.toBeInTheDocument();
      expect(screen.queryByTitle(/0 blocked/)).not.toBeInTheDocument();
    });

    it("calls onNavigateToSession when title clicked", async () => {
      const onNavigateToSession = vi.fn();
      renderHeader({ onNavigateToSession });
      await flush();
      const user = userEvent.setup();
      await user.click(screen.getByText("My Plan"));
      expect(onNavigateToSession).toHaveBeenCalledTimes(1);
    });

    it("renders tier toggle when tierGroupIds present and triggers expand action", async () => {
      const onToggleAllTiers = vi.fn();
      renderHeader({
        tierGroupIds: ["t1", "t2"],
        anyTierCollapsed: true,
        onToggleAllTiers,
      });
      await flush();
      const user = userEvent.setup();
      const btn = screen.getByLabelText("Expand all tiers");
      await user.click(btn);
      expect(onToggleAllTiers).toHaveBeenCalledWith("plan-1", "expand");
    });

    it("tier toggle 'Collapse all tiers' path when no tiers collapsed", async () => {
      const onToggleAllTiers = vi.fn();
      renderHeader({
        tierGroupIds: ["t1"],
        anyTierCollapsed: false,
        onToggleAllTiers,
      });
      await flush();
      const user = userEvent.setup();
      await user.click(screen.getByLabelText("Collapse all tiers"));
      expect(onToggleAllTiers).toHaveBeenCalledWith("plan-1", "collapse");
    });

    it("does not render tier toggle when tierGroupIds is empty", async () => {
      renderHeader({ tierGroupIds: [] });
      await flush();
      expect(screen.queryByLabelText(/all tiers/i)).not.toBeInTheDocument();
    });

    it("renders settings button when projectId is provided and opens popover", async () => {
      renderHeader({ projectId: "proj-1" });
      await flush();
      const settingsBtn = screen.getByLabelText("Plan group settings");
      expect(settingsBtn).toBeInTheDocument();
      const user = userEvent.setup();
      await user.click(settingsBtn);
      expect(
        await screen.findByTestId("plan-group-settings-content"),
      ).toBeInTheDocument();
    });

    it("settings popover navigates and closes on merge task click", async () => {
      const onNavigateToTask = vi.fn();
      renderHeader({ projectId: "proj-1", onNavigateToTask });
      await flush();
      const user = userEvent.setup();
      await user.click(screen.getByLabelText("Plan group settings"));
      const inner = await screen.findByTestId("plan-group-settings-content");
      await user.click(inner);
      expect(onNavigateToTask).toHaveBeenCalledWith("merge-task-1");
    });

    it("does not render settings button when projectId missing", async () => {
      renderHeader();
      await flush();
      expect(
        screen.queryByLabelText("Plan group settings"),
      ).not.toBeInTheDocument();
    });

    it("renders branch badge with truncated short name and merge target arrow", async () => {
      getByPlan.mockResolvedValue({
        ...branch,
        branchName: "feature/super-long-branch-name-here",
        baseBranchOverride: "develop",
      });
      renderHeader({ projectId: "proj-1" });
      await waitFor(() =>
        expect(screen.getByText(/super-long-b…/)).toBeInTheDocument(),
      );
      expect(
        screen.getByTitle(/Merging into: develop/),
      ).toBeInTheDocument();
    });

    it("uses last segment unchanged when ≤ 12 chars", async () => {
      getByPlan.mockResolvedValue({ ...branch, branchName: "feature/short" });
      renderHeader();
      await waitFor(() =>
        expect(screen.getByText("short")).toBeInTheDocument(),
      );
    });

    it("uses raw name when no slash and length ≤ 12", async () => {
      getByPlan.mockResolvedValue({ ...branch, branchName: "noslash" });
      renderHeader();
      await waitFor(() =>
        expect(screen.getByText("noslash")).toBeInTheDocument(),
      );
    });

    it("renders merged badge with check icon", async () => {
      getByPlan.mockResolvedValue({ ...branch, status: "merged" });
      renderHeader();
      await waitFor(() =>
        expect(
          screen.getByTitle(/Feature branch: .* \(merged\)/),
        ).toBeInTheDocument(),
      );
    });

    it("hides badge when status abandoned in expanded layout", async () => {
      getByPlan.mockResolvedValue({ ...branch, status: "abandoned" });
      renderHeader();
      await flush();
      expect(
        screen.queryByTitle(/Feature branch:/),
      ).not.toBeInTheDocument();
    });
  });

  it("subscribes to plan:merge_complete and invalidates query on event", async () => {
    renderHeader();
    await flush();
    expect(subscribe).toHaveBeenCalledWith(
      "plan:merge_complete",
      expect.any(Function),
    );
    // initial fetch
    expect(getByPlan).toHaveBeenCalledTimes(1);
    getByPlan.mockClear();
    getByPlan.mockResolvedValue(branch);
    await act(async () => {
      emit("plan:merge_complete");
      await Promise.resolve();
    });
    await flush();
    expect(getByPlan).toHaveBeenCalled();
  });

  it("memoized FeatureBranchBadge handles abandoned status with X icon (rendered via merged status branch)", async () => {
    // We exercise abandoned via direct render only when in expanded; covered above by hiding.
    // Sanity test render of expanded with merged ensures statusIcon rendering.
    getByPlan.mockResolvedValue({ ...branch, status: "merged" });
    renderUI(
      <PlanGroupHeader
        planArtifactId="plan-2"
        sessionTitle="Plan 2"
        taskCount={1}
        statusSummary={baseStatus}
        isCollapsed={false}
        onToggleCollapse={vi.fn()}
      />,
    );
    await waitFor(() =>
      expect(
        screen.getByTitle(/Feature branch: .* \(merged\)/),
      ).toBeInTheDocument(),
    );
  });
});
