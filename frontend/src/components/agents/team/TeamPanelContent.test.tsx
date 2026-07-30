import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  useQuery: vi.fn(),
  useManagedTeamStatus: vi.fn(),
  useEnsureManagedTeam: vi.fn(),
}));

vi.mock("@tanstack/react-query", () => ({ useQuery: mocks.useQuery }));
vi.mock("@/hooks/useManagedTeam", () => ({
  useManagedTeamStatus: mocks.useManagedTeamStatus,
  useEnsureManagedTeam: mocks.useEnsureManagedTeam,
  useManagedTeamMemberActions: () => ({
    addMember: { isPending: false, mutateAsync: vi.fn() },
    assignMember: { isPending: false, mutateAsync: vi.fn() },
    stopMember: { isPending: false, mutateAsync: vi.fn() },
    exit: { isPending: false, mutateAsync: vi.fn() },
  }),
}));

import { TeamPanelContent } from "./TeamPanelContent";

const team = {
  session: { id: "team-1", status: "active", effectiveConcurrency: 2, configuredConcurrency: 3 },
  members: [],
  usage: { tokens: 0, costMicros: 0, members: [] },
};

describe("TeamPanelContent board status", () => {
  beforeEach(() => {
    mocks.useManagedTeamStatus.mockReturnValue({ data: team, isLoading: false, isSuccess: true, isError: false });
    mocks.useEnsureManagedTeam.mockReturnValue({ mutate: vi.fn(), isPending: false });
    mocks.useQuery.mockReset();
  });

  it("does not report zero active board tasks while the board is loading", () => {
    mocks.useQuery.mockReturnValue({ isLoading: true, isError: false });

    render(<TeamPanelContent conversationId="conversation-1" projectId="project-1" activeAgentRunId={null} />);

    expect(screen.getByText(/Board loading/)).toBeInTheDocument();
    expect(screen.queryByText(/0 active board tasks/)).not.toBeInTheDocument();
  });

  it("reports an unavailable board when its task query fails", () => {
    mocks.useQuery.mockReturnValue({ isLoading: false, isError: true });

    render(<TeamPanelContent conversationId="conversation-1" projectId="project-1" activeAgentRunId={null} />);

    expect(screen.getByText(/Board unavailable/)).toBeInTheDocument();
    expect(screen.queryByText(/0 active board tasks/)).not.toBeInTheDocument();
  });
});
