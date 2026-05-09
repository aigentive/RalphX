/**
 * VerificationPanel.test.tsx
 *
 * Covers the page-load hydration fix:
 * - Tab populates when session.verificationStatus is "unverified" but backend has data
 * - Empty state renders correctly when no verification has been run (404)
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import type { IdeationSession } from "@/types/ideation";

// ── Mock React Query (avoid two-React-copies issue from mixed node_modules) ──

const mockSetQueryData = vi.fn();
const mockUseQueryClient = vi.fn(() => ({ setQueryData: mockSetQueryData }));
let mockQueryResult: { data: unknown; isLoading: boolean } = { data: undefined, isLoading: false };

vi.mock("@tanstack/react-query", () => ({
  useQuery: vi.fn(() => mockQueryResult),
  useQueryClient: mockUseQueryClient,
}));

// ── Mock API ─────────────────────────────────────────────────────────────────

vi.mock("@/api/ideation", () => ({
  ideationApi: {
    verification: {
      getStatus: vi.fn(),
      skip: vi.fn(),
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
    sendAgentMessage: vi.fn(),
  },
}));

// Mock useChildSessionStatus — added in Gap 3 fix; uses useQuery internally, which would
// consume a mock return value ahead of the component's own useQuery calls and shift the sequence.
vi.mock("@/hooks/useChildSessionStatus", () => ({
  useChildSessionStatus: vi.fn(() => ({ lastEffectiveModel: null })),
}));

const mockSetActiveVerificationChildId = vi.fn();
const mockSetLastVerificationChildId = vi.fn();

// Default store state — both IDs null (fresh mount)
let mockStoreState: Record<string, unknown> = {
  activeVerificationChildId: {},
  setActiveVerificationChildId: mockSetActiveVerificationChildId,
  lastVerificationChildId: {},
  setLastVerificationChildId: mockSetLastVerificationChildId,
  setVerificationNotification: vi.fn(),
};

vi.mock("@/stores/ideationStore", () => ({
  useIdeationStore: vi.fn((selector: (s: object) => unknown) =>
    selector(mockStoreState)
  ),
}));

// Sub-components that require their own data fetching — mock to simple divs
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

// ── Session fixture ───────────────────────────────────────────────────────────

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

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("VerificationPanel — page-load hydration", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    mockQueryResult = { data: undefined, isLoading: false };
    mockStoreState = {
      activeVerificationChildId: {},
      setActiveVerificationChildId: mockSetActiveVerificationChildId,
      lastVerificationChildId: {},
      setLastVerificationChildId: mockSetLastVerificationChildId,
      setVerificationNotification: vi.fn(),
    };
    const { useQuery } = await import("@tanstack/react-query");
    vi.mocked(useQuery).mockReturnValue(mockQueryResult as ReturnType<typeof useQuery>);
  });

  it("shows verification content (rounds) when query returns data even if session.verificationStatus is 'unverified'", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const verificationData = {
      sessionId: "session-1",
      status: "verified",
      inProgress: false,
      gaps: [],
      rounds: [{ round: 1, gapScore: 5, gapCount: 1 }],
    };
    // First call = verification data, second = childSessions (empty)
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: verificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={baseSession} />);

    await waitFor(() => {
      // Should NOT show the empty state
      expect(screen.queryByTestId("verification-empty-state")).not.toBeInTheDocument();
    });
    // Should show main content
    expect(screen.getByTestId("verification-panel-content")).toBeInTheDocument();
  });

  it("uses the canonical current verification query key", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const { verificationStatusKey } = await import("@/hooks/useVerificationStatus");
    const verificationData = {
      sessionId: "session-1",
      status: "reviewing",
      inProgress: true,
      gaps: [],
      rounds: [],
      roundDetails: [],
    };
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: verificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={baseSession} />);

    const firstQueryConfig = vi.mocked(useQuery).mock.calls[0]?.[0] as { queryKey?: unknown } | undefined;
    expect(firstQueryConfig?.queryKey).toEqual(verificationStatusKey("session-1"));
  });

  it("keeps verification history visible when only roundDetails remain after current gaps are cleared", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const verificationData = {
      sessionId: "session-1",
      status: "needs_revision",
      inProgress: false,
      gaps: [],
      rounds: [],
      roundDetails: [
        {
          round: 1,
          gapScore: 8,
          gapCount: 2,
          gaps: [
            { severity: "critical", category: "completeness", description: "Missing migration registration" },
            { severity: "high", category: "testing", description: "Missing register-project coverage" },
          ],
        },
      ],
    };
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: verificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={baseSession} />);

    await waitFor(() => {
      expect(screen.queryByTestId("verification-empty-state")).not.toBeInTheDocument();
    });
    expect(screen.getByTestId("verification-panel-content")).toBeInTheDocument();
    expect(screen.getByTestId("verification-history")).toBeInTheDocument();
  });

  it("renders the API-selected effective generation when the active generation is blank", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const currentVerificationData = {
      sessionId: "session-1",
      status: "verified",
      inProgress: false,
      generation: 2,
      selectedGeneration: 1,
      currentRound: 5,
      maxRounds: 5,
      gaps: [
        {
          severity: "medium",
          category: "completeness",
          description: "Historical verification gap",
        },
      ],
      rounds: [{ round: 4, gapScore: 1, gapCount: 1 }],
      roundDetails: [{ round: 4, gapScore: 1, gapCount: 1, gaps: [] }],
      verificationChild: {
        latestChildSessionId: "child-verifier-1",
        latestChildArchived: true,
      },
      runHistory: [
        {
          generation: 1,
          status: "verified",
          inProgress: false,
          currentRound: 5,
          maxRounds: 5,
          roundCount: 4,
          gapCount: 1,
          convergenceReason: "max_rounds",
        },
      ],
    };
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: currentVerificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    const { VerificationPanel } = await import("./VerificationPanel");
    const onDisplayedVerificationChildChange = vi.fn();
    const onDisplayedVerificationStatusChange = vi.fn();
    render(
      <VerificationPanel
        session={baseSession}
        onDisplayedVerificationChildChange={onDisplayedVerificationChildChange}
        onDisplayedVerificationStatusChange={onDisplayedVerificationStatusChange}
      />
    );

    await waitFor(() => {
      expect(screen.queryByTestId("verification-empty-state")).not.toBeInTheDocument();
    });
    expect(screen.getByTestId("verification-panel-content")).toBeInTheDocument();
    expect(screen.getByTestId("verification-run-picker-trigger")).toHaveTextContent("Previous verification");
    // Terminal status banner is gone; the gap card + history cover that.
    expect(screen.queryByTestId("verification-status-card")).not.toBeInTheDocument();
    expect(screen.getByTestId("verification-history")).toBeInTheDocument();
    await waitFor(() => {
      expect(onDisplayedVerificationChildChange).toHaveBeenCalledWith("child-verifier-1");
    });
    expect(onDisplayedVerificationStatusChange).toHaveBeenCalledWith("verified", false);
  });

  it("uses verification query status and gaps even when the session cache still says unverified", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const verificationData = {
      sessionId: "session-1",
      status: "needs_revision",
      inProgress: false,
      gaps: [
        {
          severity: "medium",
          category: "testing",
          description: "Missing sqlite repo regression",
        },
      ],
      rounds: [],
      roundDetails: [],
    };
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: verificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={baseSession} />);

    await waitFor(() => {
      expect(screen.queryByTestId("verification-empty-state")).not.toBeInTheDocument();
    });
    expect(screen.getByTestId("verification-panel-content")).toBeInTheDocument();
    expect(screen.getByTestId("address-gaps-button")).toBeInTheDocument();
    expect(screen.getByTestId("re-verify-button")).toBeInTheDocument();
  });

  it("shows empty state when query returns null (404 — no verification ever started)", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    // Both queries return null/empty
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: null } as unknown as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={baseSession} />);

    await waitFor(() => {
      expect(screen.getByTestId("verification-empty-state")).toBeInTheDocument();
    });
    // Verify First CTA shows since plan exists
    expect(screen.getByTestId("verify-first-button")).toBeInTheDocument();
  });

  it("does not show empty state when verification data is missing but a verification child session already exists", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const childSession = {
      id: "child-run-1",
      sessionPurpose: "verification",
      createdAt: "2026-01-01T00:00:00Z",
    };
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: null } as unknown as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [childSession] } as unknown as ReturnType<typeof useQuery>);

    mockStoreState = {
      ...mockStoreState,
      lastVerificationChildId: { "session-1": "child-run-1" },
    };

    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={baseSession} />);

    await waitFor(() => {
      expect(screen.queryByTestId("verification-empty-state")).not.toBeInTheDocument();
    });
    expect(screen.getByTestId("verification-panel-content")).toBeInTheDocument();
  });

  it("keeps verification history visible when a verification child session exists", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const childSession = {
      id: "child-run-1",
      sessionPurpose: "verification",
      createdAt: "2026-01-01T00:00:00Z",
    };
    const verificationData = {
      sessionId: "session-1",
      status: "needs_revision",
      inProgress: false,
      gaps: [],
      rounds: [{ round: 1, gapScore: 8, gapCount: 2 }],
      roundDetails: [{ round: 1, gapScore: 8, gapCount: 2, gaps: [] }],
    };

    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: verificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [childSession] } as unknown as ReturnType<typeof useQuery>);

    mockStoreState = {
      ...mockStoreState,
      lastVerificationChildId: { "session-1": "child-run-1" },
    };

    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={baseSession} />);

    await waitFor(() => {
      expect(screen.getByTestId("verification-history")).toBeInTheDocument();
    });
    expect(screen.getByTestId("verification-panel-content")).toBeInTheDocument();
  });

  it("shows empty state for session with no plan artifact and does not show Verify First button", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: null } as unknown as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    const sessionNoPlan: IdeationSession = { ...baseSession, planArtifactId: undefined };
    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={sessionNoPlan} />);

    await waitFor(() => {
      expect(screen.getByTestId("verification-empty-state")).toBeInTheDocument();
    });
    // No Verify First CTA without a plan artifact
    expect(screen.queryByTestId("verify-first-button")).not.toBeInTheDocument();
  });

  it("hydrates session query cache when query returns non-unverified status and session still shows unverified", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const verificationData = {
      sessionId: "session-1",
      status: "verified",
      inProgress: false,
      gaps: [],
      rounds: [{ round: 1, gapScore: 0, gapCount: 0 }],
    };
    // First call = verificationData query, second = childSessions query
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: verificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={baseSession} />);

    // The useEffect should call queryClient.setQueryData to hydrate the session cache
    await waitFor(() => {
      expect(mockSetQueryData).toHaveBeenCalledWith(
        ["session", "session-1"],
        expect.any(Function)
      );
    });
  });

  it("run selection is generation-based and does not mutate verification child store state", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const verificationData = {
      sessionId: "session-1",
      status: "reviewing",
      inProgress: true,
      generation: 20,
      currentRound: 2,
      maxRounds: 5,
      gaps: [],
      rounds: [],
      roundDetails: [],
      verificationChild: {
        latestChildSessionId: "child-run-1",
        agentState: "likely_generating",
        lastAssistantMessage: "Bootstrapping the verifier context before round 1.",
      },
      runHistory: [
        {
          generation: 20,
          status: "reviewing",
          inProgress: true,
          roundCount: 0,
          gapCount: 0,
        },
        {
          generation: 18,
          status: "unverified",
          inProgress: false,
          roundCount: 2,
          gapCount: 1,
          convergenceReason: "agent_error",
        },
      ],
    };
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: verificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: undefined } as unknown as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    const { VerificationPanel } = await import("./VerificationPanel");
    const { userEvent } = await import("@testing-library/user-event");
    const user = userEvent.setup();
    render(<VerificationPanel session={baseSession} />);

    await waitFor(() => {
      expect(screen.getByTestId("verification-run-picker-trigger")).toHaveTextContent("Current verification");
      expect(screen.getByTestId("verification-run-picker-trigger")).toHaveTextContent("Round 2 of 5");
      expect(screen.getByTestId("verification-run-picker-trigger")).not.toHaveTextContent("Current run");
    });

    await user.click(await screen.findByTestId("verification-run-picker-trigger"));

    expect(screen.getByTestId("verification-run-option-generation-20")).toHaveTextContent("Current verification");
    expect(screen.getByTestId("verification-run-option-generation-20")).toHaveTextContent("Round 2 of 5");
    expect(screen.getByTestId("verification-run-option-generation-18")).toHaveTextContent("Previous verification");
    expect(screen.getByTestId("verification-run-option-generation-18")).toHaveTextContent("Ended early");
    expect(screen.getByTestId("verification-run-picker-menu")).not.toHaveTextContent("Gen 20");
    expect(screen.getByTestId("verification-run-picker-menu")).not.toHaveTextContent("Run 1");

    await user.click(await screen.findByTestId("verification-run-option-generation-20"));

    expect(mockSetActiveVerificationChildId).not.toHaveBeenCalled();
    expect(mockSetLastVerificationChildId).not.toHaveBeenCalled();
  });

  it("keeps the current generation selected while the live run is still bootstrapping", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const currentVerificationData = {
      sessionId: "session-1",
      status: "reviewing",
      inProgress: true,
      generation: 21,
      gaps: [],
      rounds: [],
      roundDetails: [],
      verificationChild: {
        latestChildSessionId: "child-run-1",
        agentState: "likely_generating",
        lastAssistantMessage: "Bootstrapping the verifier context before round 1.",
      },
      runHistory: [
        {
          generation: 21,
          status: "reviewing",
          inProgress: true,
          roundCount: 0,
          gapCount: 0,
        },
        {
          generation: 18,
          status: "needs_revision",
          inProgress: false,
          roundCount: 2,
          gapCount: 1,
        },
      ],
    };
    const historicalVerificationData = {
      sessionId: "session-1",
      status: "needs_revision",
      inProgress: false,
      generation: 18,
      gaps: [
        {
          severity: "high",
          category: "testing",
          description: "Old historical gap that should not replace the live run",
        },
      ],
      rounds: [{ round: 1, gapScore: 4, gapCount: 1 }],
      roundDetails: [{ round: 1, gapScore: 4, gapCount: 1, gaps: [] }],
    };

    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: currentVerificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: historicalVerificationData } as unknown as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={baseSession} />);

    await waitFor(() => {
      expect(screen.getByTestId("verification-run-picker-trigger")).toHaveTextContent("Current verification");
    });
    expect(screen.getByTestId("verification-run-picker-trigger")).not.toHaveTextContent("Run 1");
    expect(screen.getByTestId("verification-run-picker-trigger")).not.toHaveTextContent("Gen 21");
    expect(screen.getByTestId("verification-current-run-bootstrap")).toBeInTheDocument();
    expect(screen.getByText("Verification is warming up")).toBeInTheDocument();
    expect(screen.getByText("Bootstrapping the verifier context before round 1.")).toBeInTheDocument();
  });

  it("auto-update effect sets activeVerificationChildId only on first mount (both null)", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const childSession = {
      id: "child-run-1",
      sessionPurpose: "verification",
      createdAt: "2026-01-01T00:00:00Z",
    };
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: null } as unknown as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: undefined } as unknown as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [childSession] } as unknown as ReturnType<typeof useQuery>);

    // Both IDs null — first mount scenario
    mockStoreState = {
      ...mockStoreState,
      activeVerificationChildId: {},
      lastVerificationChildId: {},
    };

    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={baseSession} />);

    // On first mount, both are null so activeVerificationChildId IS set
    await waitFor(() => {
      expect(mockSetActiveVerificationChildId).toHaveBeenCalledWith("session-1", "child-run-1");
    });
    expect(mockSetLastVerificationChildId).toHaveBeenCalledWith("session-1", "child-run-1");
  });

  // ── isInProgress dual-source derivation ───────────────────────────────────

  it("isInProgress: false when both session.verificationInProgress=false and activeVerificationChildId=null", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const verificationData = { sessionId: "session-1", status: "needs_revision", inProgress: false, gaps: [{ description: "gap1" }], rounds: [{ round: 1, gapScore: 3, gapCount: 1 }] };
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: verificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    mockStoreState = { ...mockStoreState, activeVerificationChildId: {}, lastVerificationChildId: {} };
    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={{ ...baseSession, verificationStatus: "needs_revision" }} />);

    // When neither source is active, Address Gaps and Re-verify should be visible
    await waitFor(() => {
      expect(screen.getByTestId("address-gaps-button")).toBeInTheDocument();
    });
    expect(screen.getByTestId("re-verify-button")).toBeInTheDocument();
  });

  it("isInProgress: true when session.verificationInProgress=true and activeVerificationChildId=null", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const verificationData = { sessionId: "session-1", status: "needs_revision", inProgress: true, gaps: [{ description: "gap1" }], rounds: [{ round: 1, gapScore: 3, gapCount: 1 }] };
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: verificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    mockStoreState = { ...mockStoreState, activeVerificationChildId: {}, lastVerificationChildId: {} };
    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={{ ...baseSession, verificationStatus: "needs_revision", verificationInProgress: true }} />);

    // When session.verificationInProgress=true, Address Gaps and Re-verify should be hidden
    await waitFor(() => {
      expect(screen.queryByTestId("address-gaps-button")).not.toBeInTheDocument();
    });
    expect(screen.queryByTestId("re-verify-button")).not.toBeInTheDocument();
  });

  it("isInProgress: true when session.verificationInProgress=false but activeVerificationChildId is set", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const verificationData = { sessionId: "session-1", status: "needs_revision", inProgress: false, gaps: [{ description: "gap1" }], rounds: [{ round: 1, gapScore: 3, gapCount: 1 }] };
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: verificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    // activeVerificationChildId set — this is the key dual-source fix
    mockStoreState = { ...mockStoreState, activeVerificationChildId: { "session-1": "child-123" }, lastVerificationChildId: {} };
    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={{ ...baseSession, verificationStatus: "needs_revision", verificationInProgress: false }} />);

    // activeVerificationChildId alone makes isInProgress=true, so Address Gaps and Re-verify hidden
    await waitFor(() => {
      expect(screen.queryByTestId("address-gaps-button")).not.toBeInTheDocument();
    });
    expect(screen.queryByTestId("re-verify-button")).not.toBeInTheDocument();
  });

  it("isInProgress: true when both session.verificationInProgress=true AND activeVerificationChildId set", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const verificationData = { sessionId: "session-1", status: "needs_revision", inProgress: true, gaps: [{ description: "gap1" }], rounds: [{ round: 1, gapScore: 3, gapCount: 1 }] };
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: verificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    mockStoreState = { ...mockStoreState, activeVerificationChildId: { "session-1": "child-123" }, lastVerificationChildId: {} };
    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={{ ...baseSession, verificationStatus: "needs_revision", verificationInProgress: true }} />);

    await waitFor(() => {
      expect(screen.queryByTestId("address-gaps-button")).not.toBeInTheDocument();
    });
    expect(screen.queryByTestId("re-verify-button")).not.toBeInTheDocument();
  });

  // ── Hydration effect: apiInProgress + new child ───────────────────────────

  it("hydration effect sets activeVerificationChildId when apiInProgress=true and latestId differs from lastVerificationChildId", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const childSession = { id: "child-new", sessionPurpose: "verification", createdAt: "2026-01-01T01:00:00Z" };
    const verificationData = { sessionId: "session-1", status: "reviewing", inProgress: true, gaps: [], rounds: [] };
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: verificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: undefined } as unknown as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [childSession] } as unknown as ReturnType<typeof useQuery>);

    // activeVerificationChildId=null, lastVerificationChildId="child-old" (terminated child)
    // latestId="child-new" !== "child-old" → hydration should fire
    mockStoreState = {
      ...mockStoreState,
      activeVerificationChildId: {},
      lastVerificationChildId: { "session-1": "child-old" },
    };

    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={baseSession} />);

    await waitFor(() => {
      expect(mockSetActiveVerificationChildId).toHaveBeenCalledWith("session-1", "child-new");
    });
  });

  it("hydration effect does NOT re-assert activeVerificationChildId when latestId equals lastVerificationChildId (termination guard)", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const childSession = { id: "child-terminated", sessionPurpose: "verification", createdAt: "2026-01-01T01:00:00Z" };
    // stale verificationData still shows inProgress=true but the same child terminated
    const verificationData = { sessionId: "session-1", status: "reviewing", inProgress: true, gaps: [], rounds: [] };
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: verificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: undefined } as unknown as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [childSession] } as unknown as ReturnType<typeof useQuery>);

    // activeVerificationChildId=null (cleared by termination), lastVerificationChildId="child-terminated"
    // latestId="child-terminated" === lastVerificationChildId → hydration must NOT fire
    mockStoreState = {
      ...mockStoreState,
      activeVerificationChildId: {},
      lastVerificationChildId: { "session-1": "child-terminated" },
    };

    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={baseSession} />);

    await new Promise((r) => setTimeout(r, 50));
    expect(mockSetActiveVerificationChildId).not.toHaveBeenCalled();
  });

  it("does NOT hydrate session cache when session already has a non-unverified status", async () => {
    const { useQuery } = await import("@tanstack/react-query");
    const verificationData = {
      sessionId: "session-1",
      status: "verified",
      inProgress: false,
      gaps: [],
      rounds: [{ round: 1, gapScore: 0, gapCount: 0 }],
    };
    vi.mocked(useQuery)
      .mockReturnValueOnce({ data: verificationData } as ReturnType<typeof useQuery>)
      .mockReturnValueOnce({ data: [] } as unknown as ReturnType<typeof useQuery>);

    // Session already has a non-unverified status — hydration should be skipped
    const sessionAlreadyVerified: IdeationSession = {
      ...baseSession,
      verificationStatus: "verified",
    };
    const { VerificationPanel } = await import("./VerificationPanel");
    render(<VerificationPanel session={sessionAlreadyVerified} />);

    await new Promise((r) => setTimeout(r, 50));
    expect(mockSetQueryData).not.toHaveBeenCalled();
  });
});
