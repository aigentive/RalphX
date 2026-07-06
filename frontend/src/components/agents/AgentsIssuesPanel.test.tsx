import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentConversationIssue } from "@/api/chat";

import { AgentsIssuesPanel } from "./AgentsIssuesPanel";

const {
  listIssuesMock,
  convertIssueMock,
  updateIssueStatusMock,
  toastErrorMock,
  toastSuccessMock,
} = vi.hoisted(() => ({
  listIssuesMock: vi.fn(),
  convertIssueMock: vi.fn(),
  updateIssueStatusMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastSuccessMock: vi.fn(),
}));

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      listAgentConversationIssues: (...args: unknown[]) => listIssuesMock(...args),
      convertAgentConversationIssueFollowup: (...args: unknown[]) =>
        convertIssueMock(...args),
      updateAgentConversationIssueStatus: (...args: unknown[]) =>
        updateIssueStatusMock(...args),
    },
  };
});

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

function issue(overrides: Partial<AgentConversationIssue> = {}): AgentConversationIssue {
  return {
    id: "issue-1",
    projectId: "project-1",
    conversationId: "conversation-1",
    sourceTaskId: "task-1",
    sourceContextType: "review",
    sourceContextId: "review-1",
    sourceAgentName: "ralphx-execution-reviewer",
    issueKind: "plan_drift",
    severity: "critical",
    status: "open",
    blockingScope: "followup_only",
    title: "Unrelated drift",
    summary: "The task found unrelated work.",
    evidence: "src/unrelated.rs changed",
    recommendation: "Create a separate follow-up.",
    blockerFingerprint: "scope:task-1",
    canonicalFingerprint: "v1:scope-drift:task:task-1:files:abc123",
    canonicalScopeKind: "task",
    canonicalScopeSubject: "task-1",
    canonicalFamily: "scope-drift",
    supersededByIssueId: null,
    occurrenceCount: 1,
    occurrences: [
      {
        id: "occurrence-1",
        issueId: "issue-1",
        sourceTaskId: "task-1",
        sourceContextType: "review",
        sourceContextId: "review-1",
        sourceAgentName: "ralphx-execution-reviewer",
        issueKind: "plan_drift",
        severity: "critical",
        blockingScope: "followup_only",
        title: "Unrelated drift",
        summary: "The task found unrelated work.",
        evidence: "src/unrelated.rs changed",
        recommendation: "Create a separate follow-up.",
        rawBlockerFingerprint: "scope:task-1",
        canonicalFingerprint: "v1:scope-drift:task:task-1:files:abc123",
        dedupeDecision: "created",
        createdAt: "2026-06-25T12:01:00Z",
      },
    ],
    followupTitle: "Investigate unrelated drift",
    followupPrompt: "Plan the unrelated work separately.",
    autoFollowupEligible: true,
    linkedFollowupConversationId: null,
    createdAt: "2026-06-25T12:00:00Z",
    updatedAt: "2026-06-25T12:01:00Z",
    resolvedAt: null,
    ...overrides,
  };
}

function renderPanel(conversationId: string | null = "conversation-1") {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <AgentsIssuesPanel conversationId={conversationId} projectId="project-1" />
    </QueryClientProvider>,
  );
}

describe("AgentsIssuesPanel", () => {
  beforeEach(() => {
    listIssuesMock.mockReset();
    convertIssueMock.mockReset();
    updateIssueStatusMock.mockReset();
    toastErrorMock.mockReset();
    toastSuccessMock.mockReset();
  });

  it("renders empty states without fetching when no conversation is selected", () => {
    renderPanel(null);

    expect(screen.getByText("No conversation selected")).toBeInTheDocument();
    expect(listIssuesMock).not.toHaveBeenCalled();
  });

  it("renders issues and dispatches follow-up and status mutations", async () => {
    const user = userEvent.setup();
    const baseOccurrence = issue().occurrences[0];
    const openIssue = issue({
      occurrenceCount: 2,
      occurrences: [
        baseOccurrence,
        {
          ...baseOccurrence,
          id: "occurrence-2",
          sourceAgentName: "ralphx-execution-worker",
          summary: "Worker found the same blocker.",
          createdAt: "2026-06-25T12:02:00Z",
        },
      ],
    });
    const linkedIssue = issue({
      id: "issue-2",
      severity: "low",
      title: "Decision needed",
      summary: "The agent needs user direction.",
      sourceTaskId: null,
      sourceAgentName: null,
      blockerFingerprint: null,
      canonicalFingerprint: null,
      canonicalScopeKind: null,
      canonicalScopeSubject: null,
      canonicalFamily: null,
      occurrenceCount: null,
      occurrences: [],
      evidence: null,
      recommendation: null,
      updatedAt: "not-a-date",
      linkedFollowupConversationId: "followup-conversation-1",
    });
    listIssuesMock.mockResolvedValue([openIssue, linkedIssue]);
    convertIssueMock.mockResolvedValue(
      issue({ linkedFollowupConversationId: "followup-conversation-2" }),
    );
    updateIssueStatusMock.mockResolvedValue(issue({ status: "resolved" }));

    renderPanel();

    expect(await screen.findByText("2 open issues")).toBeInTheDocument();
    expect(screen.getByText("Unrelated drift")).toBeInTheDocument();
    expect(screen.getByText("Decision needed")).toBeInTheDocument();
    expect(screen.getByText("Critical")).toBeInTheDocument();
    expect(screen.getAllByText("Followup Only")).toHaveLength(2);
    expect(screen.getByText("2 reports")).toBeInTheDocument();
    expect(screen.getByText("Reports")).toBeInTheDocument();
    expect(screen.getByText("Worker found the same blocker.")).toBeInTheDocument();
    expect(screen.getByText("v1:scope-drift:task:task-1:files:abc123")).toBeInTheDocument();
    expect(screen.getByText("src/unrelated.rs changed")).toBeInTheDocument();
    expect(screen.getByText("Create a separate follow-up.")).toBeInTheDocument();
    expect(screen.getByText("not-a-date")).toBeInTheDocument();
    expect(screen.getByText("Follow-up created")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /create follow-up/i }));
    await waitFor(() => expect(convertIssueMock).toHaveBeenCalledWith("issue-1"));
    expect(toastSuccessMock).toHaveBeenCalledWith("Follow-up Agent conversation ready");

    await user.click(screen.getAllByRole("button", { name: /resolve/i })[0]);
    await waitFor(() =>
      expect(updateIssueStatusMock).toHaveBeenCalledWith("issue-1", "resolved"),
    );

    await user.click(screen.getAllByRole("button", { name: /dismiss/i })[0]);
    await waitFor(() =>
      expect(updateIssueStatusMock).toHaveBeenCalledWith("issue-1", "dismissed"),
    );
  });

  it("surfaces mutation failures", async () => {
    const user = userEvent.setup();
    listIssuesMock.mockResolvedValue([issue()]);
    convertIssueMock.mockRejectedValue(new Error("follow-up failed"));
    updateIssueStatusMock.mockRejectedValue(new Error("status failed"));

    renderPanel();

    await screen.findByText("1 open issue");
    await user.click(screen.getByRole("button", { name: /create follow-up/i }));

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalledWith("follow-up failed"));

    await user.click(screen.getByRole("button", { name: /resolve/i }));
    await waitFor(() => expect(toastErrorMock).toHaveBeenCalledWith("status failed"));
  });
});
