/**
 * VerificationPanel.branches.test.tsx
 *
 * Targets uncovered branches: action handlers, query options (queryFn / retry /
 * retryDelay), VerificationRunPicker outside-click + Escape, stale-verification
 * banner, single-run picker label fallback, helper edge cases.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent, act } from "@testing-library/react";
import type { IdeationSession } from "@/types/ideation";

// ── Mock React Query ─────────────────────────────────────────────────────────

const mockSetQueryData = vi.fn();
const mockInvalidateQueries = vi.fn();
const mockUseQueryClient = vi.fn(() => ({
  setQueryData: mockSetQueryData,
  invalidateQueries: mockInvalidateQueries,
}));

vi.mock("@tanstack/react-query", () => ({
  useQuery: vi.fn(() => ({ data: undefined, isLoading: false })),
  useQueryClient: mockUseQueryClient,
}));

// ── Mock API ─────────────────────────────────────────────────────────────────

const mockGetStatus = vi.fn();
const mockSkip = vi.fn();
const mockSendAgentMessage = vi.fn();

vi.mock("@/api/ideation", () => ({
  ideationApi: {
    verification: {
      getStatus: (...args: unknown[]) => mockGetStatus(...args),
      skip: (...args: unknown[]) => mockSkip(...args),
    },
    sessions: {
      getChildren: vi.fn().mockResolvedValue([]),
    },
  },
}));

vi.mock("@/hooks/useIdeation", () => ({
  ideationKeys: {
    sessions: () => ["sessions"],
    sessionWithData: (id: string) => ["session", id],
  },
}));

vi.mock("@/api/chat", () => ({
  chatApi: {
    sendAgentMessage: (...args: unknown[]) => mockSendAgentMessage(...args),
  },
}));

vi.mock("@/hooks/useChildSessionStatus", () => ({
  useChildSessionStatus: vi.fn(() => ({ lastEffectiveModel: null })),
}));

const mockEnqueuePendingVerification = vi.fn();
vi.mock("@/stores/uiStore", () => ({
  useUiStore: vi.fn((selector: (s: object) => unknown) =>
    selector({ enqueuePendingVerification: mockEnqueuePendingVerification }),
  ),
}));

vi.mock("@/stores/chatStore", () => ({
  useChatStore: Object.assign(
    vi.fn((selector: (s: object) => unknown) =>
      selector({ effectiveModel: {}, setEffectiveModel: vi.fn() }),
    ),
    {
      getState: () => ({ effectiveModel: {}, setEffectiveModel: vi.fn() }),
    },
  ),
}));

const mockSetActiveVerificationChildId = vi.fn();
const mockSetLastVerificationChildId = vi.fn();

let mockStoreState: Record<string, unknown> = {
  activeVerificationChildId: {},
  setActiveVerificationChildId: mockSetActiveVerificationChildId,
  lastVerificationChildId: {},
  setLastVerificationChildId: mockSetLastVerificationChildId,
  setVerificationNotification: vi.fn(),
};

vi.mock("@/stores/ideationStore", () => ({
  useIdeationStore: vi.fn((selector: (s: object) => unknown) =>
    selector(mockStoreState),
  ),
}));

vi.mock("./VerificationBadge", () => ({
  VerificationBadge: ({ status }: { status: string }) => (
    <div data-testid="verification-badge">{status}</div>
  ),
}));

vi.mock("./VerificationGapList", () => ({
  VerificationGapList: () => <div data-testid="verification-gap-list" />,
}));

vi.mock("./VerificationHistory", () => ({
  VerificationHistory: () => <div data-testid="verification-history" />,
}));

// ── Session fixture ─────────────────────────────────────────────────────────

const baseSession: IdeationSession = {
  id: "session-1",
  projectId: "proj-1",
  title: "My Plan",
  status: "active",
  sessionPurpose: "ideation",
  verificationStatus: "unverified",
  verificationInProgress: false,
  planArtifactId: "artifact-1",
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
};

async function importPanel() {
  const mod = await import("./VerificationPanel");
  return mod.VerificationPanel;
}

async function setUseQuery(...returns: Array<{ data: unknown; isLoading?: boolean }>) {
  const { useQuery } = await import("@tanstack/react-query");
  vi.mocked(useQuery).mockReset();
  for (const r of returns) {
    vi.mocked(useQuery).mockReturnValueOnce(r as ReturnType<typeof useQuery>);
  }
  // Fallback for any extra calls
  vi.mocked(useQuery).mockReturnValue({ data: undefined } as ReturnType<typeof useQuery>);
}

beforeEach(() => {
  vi.clearAllMocks();
  mockStoreState = {
    activeVerificationChildId: {},
    setActiveVerificationChildId: mockSetActiveVerificationChildId,
    lastVerificationChildId: {},
    setLastVerificationChildId: mockSetLastVerificationChildId,
    setVerificationNotification: vi.fn(),
  };
});

describe("VerificationPanel — handler branches", () => {
  it("Verify First button enqueues pending verification (empty state)", async () => {
    await setUseQuery({ data: null }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={baseSession} />);
    fireEvent.click(await screen.findByTestId("verify-first-button"));
    expect(mockEnqueuePendingVerification).toHaveBeenCalledWith("session-1");
  });

  it("Skip button (empty state) calls verification.skip and invalidates queries", async () => {
    mockSkip.mockResolvedValue(undefined);
    await setUseQuery({ data: null }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={baseSession} />);
    fireEvent.click(await screen.findByTestId("skip-verification-button"));
    await waitFor(() => {
      expect(mockSkip).toHaveBeenCalledWith("session-1");
    });
    expect(mockSetQueryData).toHaveBeenCalled();
    expect(mockInvalidateQueries).toHaveBeenCalled();
  });

  it("Skip button toasts and invalidates on API failure", async () => {
    mockSkip.mockRejectedValue(new Error("boom"));
    const errSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    await setUseQuery({ data: null }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={baseSession} />);
    fireEvent.click(await screen.findByTestId("skip-verification-button"));
    await waitFor(() => {
      expect(mockSkip).toHaveBeenCalled();
    });
    // Error path also invalidates the session-with-data key
    expect(mockInvalidateQueries).toHaveBeenCalled();
    errSpy.mockRestore();
  });

  it("Address All Gaps sends generic message when nothing selected", async () => {
    mockSendAgentMessage.mockResolvedValue(undefined);
    const verificationData = {
      sessionId: "session-1",
      status: "needs_revision",
      inProgress: false,
      gaps: [
        { description: "missing migration" },
        { description: "missing test" },
      ],
      rounds: [{ round: 1, gapScore: 5, gapCount: 2 }],
      roundDetails: [{ round: 1, gapScore: 5, gapCount: 2, gaps: [] }],
    };
    await setUseQuery({ data: verificationData }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={{ ...baseSession, verificationStatus: "needs_revision" }} />);
    fireEvent.click(await screen.findByTestId("address-gaps-button"));
    await waitFor(() => {
      expect(mockSendAgentMessage).toHaveBeenCalledWith(
        "ideation",
        "session-1",
        "update the plan to address all verification gaps",
      );
    });
  });

  it("Address Gaps logs error when sendAgentMessage rejects", async () => {
    mockSendAgentMessage.mockRejectedValue(new Error("nope"));
    const errSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const verificationData = {
      sessionId: "session-1",
      status: "needs_revision",
      inProgress: false,
      gaps: [{ description: "g1" }],
      rounds: [{ round: 1, gapScore: 1, gapCount: 1 }],
      roundDetails: [{ round: 1, gapScore: 1, gapCount: 1, gaps: [] }],
    };
    await setUseQuery({ data: verificationData }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={{ ...baseSession, verificationStatus: "needs_revision" }} />);
    fireEvent.click(await screen.findByTestId("address-gaps-button"));
    await waitFor(() => {
      expect(errSpy).toHaveBeenCalled();
    });
    errSpy.mockRestore();
  });

  it("Re-verify button enqueues pending verification on terminal needs_revision", async () => {
    const verificationData = {
      sessionId: "session-1",
      status: "needs_revision",
      inProgress: false,
      gaps: [{ description: "g1" }],
      rounds: [{ round: 1, gapScore: 1, gapCount: 1 }],
      roundDetails: [{ round: 1, gapScore: 1, gapCount: 1, gaps: [] }],
    };
    await setUseQuery({ data: verificationData }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={{ ...baseSession, verificationStatus: "needs_revision" }} />);
    fireEvent.click(await screen.findByTestId("re-verify-button"));
    expect(mockEnqueuePendingVerification).toHaveBeenCalledWith("session-1");
  });
});

describe("VerificationPanel — query option branches", () => {
  it("queryFn returns null when API throws a 404 Error", async () => {
    await setUseQuery({ data: undefined }, { data: undefined }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={baseSession} />);

    const { useQuery } = await import("@tanstack/react-query");
    const config = vi.mocked(useQuery).mock.calls[0]?.[0] as {
      queryFn: () => Promise<unknown>;
      retry: (n: number, err: unknown) => boolean;
      retryDelay: (n: number) => number;
    };

    mockGetStatus.mockRejectedValueOnce(new Error("Request failed: 404"));
    await expect(config.queryFn()).resolves.toBeNull();

    // Non-404 errors propagate
    mockGetStatus.mockRejectedValueOnce(new Error("500 server error"));
    await expect(config.queryFn()).rejects.toThrow("500 server error");

    // Happy path returns data
    mockGetStatus.mockResolvedValueOnce({ status: "verified" });
    await expect(config.queryFn()).resolves.toEqual({ status: "verified" });

    // retry: 404 → false; non-Error → fallback to failureCount<2
    expect(config.retry(0, new Error("404 Not Found"))).toBe(false);
    expect(config.retry(0, new Error("500"))).toBe(true);
    expect(config.retry(2, new Error("500"))).toBe(false);
    expect(config.retry(0, "not-an-error")).toBe(true);

    // retryDelay caps at 10000 and grows with attempt
    expect(config.retryDelay(0)).toBe(1000);
    expect(config.retryDelay(2)).toBe(4000);
    expect(config.retryDelay(20)).toBe(10000);
  });

  it("historical query options (404 retry / retryDelay) cover the same branches", async () => {
    // Need historical query to be the second call
    const currentData = {
      sessionId: "session-1",
      status: "needs_revision",
      inProgress: false,
      generation: 3,
      selectedGeneration: 1,
      gaps: [],
      rounds: [],
      roundDetails: [],
      runHistory: [
        { generation: 1, status: "needs_revision", inProgress: false, roundCount: 1, gapCount: 0 },
        { generation: 3, status: "needs_revision", inProgress: false, roundCount: 1, gapCount: 0 },
      ],
    };
    await setUseQuery({ data: currentData }, { data: undefined }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={baseSession} />);

    const { useQuery } = await import("@tanstack/react-query");
    const histConfig = vi.mocked(useQuery).mock.calls[1]?.[0] as {
      queryFn: () => Promise<unknown>;
      retry: (n: number, err: unknown) => boolean;
      retryDelay: (n: number) => number;
    };

    mockGetStatus.mockResolvedValueOnce({ status: "verified" });
    await expect(histConfig.queryFn()).resolves.toEqual({ status: "verified" });

    expect(histConfig.retry(0, new Error("404"))).toBe(false);
    expect(histConfig.retry(0, new Error("500"))).toBe(true);
    expect(histConfig.retry(3, new Error("500"))).toBe(false);
    expect(histConfig.retryDelay(0)).toBe(1000);
    expect(histConfig.retryDelay(20)).toBe(10000);
  });
});

describe("VerificationPanel — VerificationRunPicker dropdown", () => {
  function multiRunData() {
    return {
      sessionId: "session-1",
      status: "needs_revision",
      inProgress: false,
      generation: 3,
      gaps: [],
      rounds: [],
      roundDetails: [],
      runHistory: [
        { generation: 1, status: "needs_revision", inProgress: false, roundCount: 2, gapCount: 1 },
        { generation: 2, status: "verified", inProgress: false, roundCount: 3, gapCount: 0 },
        { generation: 3, status: "needs_revision", inProgress: false, roundCount: 1, gapCount: 1 },
      ],
    };
  }

  it("opens dropdown, selects another run, and closes after selection", async () => {
    await setUseQuery({ data: multiRunData() }, { data: undefined }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={baseSession} />);

    const trigger = await screen.findByTestId("verification-run-picker-trigger");
    fireEvent.click(trigger);
    const menu = await screen.findByTestId("verification-run-picker-menu");
    expect(menu).toBeInTheDocument();

    // Hover one of the option buttons to exercise mouseEnter/mouseLeave handlers
    const opt2 = screen.getByTestId("verification-run-option-generation-2");
    fireEvent.mouseEnter(opt2);
    fireEvent.mouseLeave(opt2);

    // Hover trigger
    fireEvent.mouseEnter(trigger);
    fireEvent.mouseLeave(trigger);

    fireEvent.click(opt2);
    await waitFor(() => {
      expect(screen.queryByTestId("verification-run-picker-menu")).not.toBeInTheDocument();
    });
  });

  it("closes dropdown when Escape key pressed", async () => {
    await setUseQuery({ data: multiRunData() }, { data: undefined }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={baseSession} />);

    const trigger = await screen.findByTestId("verification-run-picker-trigger");
    fireEvent.click(trigger);
    expect(await screen.findByTestId("verification-run-picker-menu")).toBeInTheDocument();

    act(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    await waitFor(() => {
      expect(screen.queryByTestId("verification-run-picker-menu")).not.toBeInTheDocument();
    });
  });

  it("closes dropdown on outside mousedown", async () => {
    await setUseQuery({ data: multiRunData() }, { data: undefined }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={baseSession} />);

    const trigger = await screen.findByTestId("verification-run-picker-trigger");
    fireEvent.click(trigger);
    expect(await screen.findByTestId("verification-run-picker-menu")).toBeInTheDocument();

    // Dispatch a mousedown outside the picker
    act(() => {
      document.dispatchEvent(
        new MouseEvent("mousedown", { bubbles: true }),
      );
    });
    await waitFor(() => {
      expect(screen.queryByTestId("verification-run-picker-menu")).not.toBeInTheDocument();
    });
  });

  it("ignores Escape and outside-click when menu is closed (no-op branches)", async () => {
    await setUseQuery({ data: multiRunData() }, { data: undefined }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={baseSession} />);

    // Menu is closed by default — listeners should early-return
    act(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
      document.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    });
    expect(screen.queryByTestId("verification-run-picker-menu")).not.toBeInTheDocument();
  });
});

describe("VerificationPanel — stale verification banner", () => {
  it("renders stale banner when in_progress for too long, retry button works", async () => {
    const veryOld = new Date(Date.now() - 1000 * 60 * 60).toISOString(); // 1h ago
    const childSession = {
      id: "child-1",
      sessionPurpose: "verification",
      createdAt: veryOld,
    };
    const verificationData = {
      sessionId: "session-1",
      status: "reviewing",
      inProgress: true,
      generation: 5,
      maxRounds: 1, // → staleThresholdMs = 5min, easily exceeded
      gaps: [],
      rounds: [],
      roundDetails: [],
      verificationChild: { latestChildSessionId: "child-1", agentState: "running" },
    };
    await setUseQuery(
      { data: verificationData },
      { data: undefined },
      { data: [childSession] },
    );
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={{ ...baseSession, verificationInProgress: true }} />);

    const stale = await screen.findByTestId("stale-verification-warning");
    expect(stale).toBeInTheDocument();

    const retryBtn = screen.getByTestId("stale-retry-button");
    fireEvent.mouseEnter(retryBtn);
    fireEvent.mouseLeave(retryBtn);
    fireEvent.click(retryBtn);
    expect(mockEnqueuePendingVerification).toHaveBeenCalledWith("session-1");
  });
});

describe("VerificationPanel — empty state Verify First hover", () => {
  it("hovering Verify First and Skip toggles backgrounds", async () => {
    await setUseQuery({ data: null }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={baseSession} />);

    const verify = await screen.findByTestId("verify-first-button");
    fireEvent.mouseEnter(verify);
    fireEvent.mouseLeave(verify);

    const skip = screen.getByTestId("skip-verification-button");
    fireEvent.mouseEnter(skip);
    fireEvent.mouseLeave(skip);
  });
});

describe("VerificationPanel — main-content button hovers", () => {
  it("hover handlers cover address-gaps, re-verify, skip ghost backgrounds", async () => {
    const verificationData = {
      sessionId: "session-1",
      status: "needs_revision",
      inProgress: false,
      gaps: [{ description: "g1" }],
      rounds: [{ round: 1, gapScore: 1, gapCount: 1 }],
      roundDetails: [{ round: 1, gapScore: 1, gapCount: 1, gaps: [] }],
    };
    await setUseQuery({ data: verificationData }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={{ ...baseSession, verificationStatus: "needs_revision" }} />);

    const addr = await screen.findByTestId("address-gaps-button");
    fireEvent.mouseEnter(addr);
    fireEvent.mouseLeave(addr);

    const reverify = screen.getByTestId("re-verify-button");
    fireEvent.mouseEnter(reverify);
    fireEvent.mouseLeave(reverify);

    const skip = screen.getByTestId("skip-verification-button");
    fireEvent.mouseEnter(skip);
    fireEvent.mouseLeave(skip);
  });
});

describe("VerificationPanel — bootstrap card with currentRound/maxRounds", () => {
  it("renders Round X/Y badge when both currentRound and maxRounds present", async () => {
    const data = {
      sessionId: "session-1",
      status: "reviewing",
      inProgress: true,
      generation: 1,
      currentRound: 2,
      maxRounds: 5,
      gaps: [],
      rounds: [],
      roundDetails: [],
      verificationChild: {
        latestChildSessionId: "child-deadbeef-0000",
        agentState: "queued",
      },
    };
    await setUseQuery({ data }, { data: undefined }, { data: [] });
    const VerificationPanel = await importPanel();
    render(<VerificationPanel session={{ ...baseSession, verificationInProgress: true }} />);

    expect(await screen.findByTestId("verification-current-run-bootstrap")).toBeInTheDocument();
    expect(screen.getByText("Round 2/5")).toBeInTheDocument();
    // Truncated child id
    expect(screen.getByText(/child-de…/)).toBeInTheDocument();
  });
});
