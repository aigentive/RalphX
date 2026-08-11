import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  ticketingApi,
  type ConversationTicket,
  type TicketDetail,
} from "@/api/ticketing";
import { TooltipProvider } from "@/components/ui/tooltip";

import { ArtifactSelectionProvider } from "./artifact-selection/ArtifactSelectionProvider";
import { AgentsClickUpIssuePanel } from "./AgentsClickUpIssuePanel";

vi.mock("@/api/ticketing", async () => {
  const actual =
    await vi.importActual<typeof import("@/api/ticketing")>("@/api/ticketing");
  return {
    ...actual,
    ticketingApi: {
      ...actual.ticketingApi,
      getConversationTicket: vi.fn(),
      getTicketDetail: vi.fn(),
    },
  };
});

const getConversationTicketMock = vi.mocked(ticketingApi.getConversationTicket);
const getTicketDetailMock = vi.mocked(ticketingApi.getTicketDetail);
type AddExcerptHandler = Parameters<
  typeof ArtifactSelectionProvider
>[0]["onAddExcerpt"];

function binding(
  overrides: Partial<ConversationTicket> = {},
): ConversationTicket {
  return {
    ticketRef: { provider: "clickup", id: "task-123", key: "CU-123" },
    projectId: "project-1",
    title: "Restore rich ClickUp details",
    url: "https://app.clickup.com/t/task-123",
    ...overrides,
  };
}

function detail(overrides: Partial<TicketDetail> = {}): TicketDetail {
  return {
    ref: { provider: "clickup", id: "task-123", key: "CU-123" },
    title: "Restore rich ClickUp details",
    state: { id: "in-progress", name: "In Progress", category: "in_progress" },
    assignee: null,
    assignees: [],
    watchers: [],
    reporter: { id: "user-1", name: "Alex" },
    labels: ["frontend"],
    sprints: [],
    project: "RalphX",
    priority: "High",
    updatedAt: "2026-07-20T12:00:00Z",
    url: "https://app.clickup.com/t/task-123",
    associationCount: 0,
    openPrCount: 0,
    currentUserAssigned: false,
    currentUserWatching: false,
    descriptionMarkdown:
      "## Expected\n\n- Show the activity feed\n\n![workflow](https://cdn.example/description.png)",
    descriptionText: "Expected: Show the activity feed",
    comments: [
      {
        id: "comment-1",
        author: { id: "user-2", name: "Morgan" },
        bodyMarkdown: "Here is the latest screenshot.",
        bodyText: "Here is the latest screenshot.",
        createdAt: "2026-07-20T13:00:00Z",
        attachments: [
          {
            id: "comment-image",
            filename: "activity.png",
            mimeType: "image/png",
            size: 1024,
            url: "https://cdn.example/activity.png",
          },
        ],
        replies: [
          {
            id: "reply-1",
            author: { id: "user-3", name: "Sam" },
            bodyMarkdown: "Confirmed in ClickUp.",
            bodyText: "Confirmed in ClickUp.",
            createdAt: "2026-07-20T14:00:00Z",
            attachments: [
              {
                id: "reply-image",
                filename: "reply.png",
                mimeType: "image/png",
                size: 2048,
                url: "https://cdn.example/reply.png",
              },
            ],
            replies: [],
          },
        ],
      },
    ],
    attachments: [
      {
        id: "task-image",
        filename: "task.png",
        mimeType: "image/png",
        size: 4096,
        url: "https://cdn.example/task.png",
      },
    ],
    transitions: [],
    ...overrides,
  };
}

function renderPanel({
  onAddExcerpt,
}: {
  onAddExcerpt?: AddExcerptHandler;
} = {}) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });
  const content = <AgentsClickUpIssuePanel conversationId="conversation-1" />;
  const wrappedContent = onAddExcerpt ? (
    <ArtifactSelectionProvider enabled onAddExcerpt={onAddExcerpt}>
      {content}
    </ArtifactSelectionProvider>
  ) : (
    content
  );

  render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>{wrappedContent}</TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("AgentsClickUpIssuePanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getConversationTicketMock.mockResolvedValue(binding());
    getTicketDetailMock.mockResolvedValue(detail());
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders ClickUp description images, task attachments, comments, replies, and comment attachments", async () => {
    renderPanel();

    expect(
      await screen.findByRole("heading", { name: "Expected" }),
    ).toBeInTheDocument();
    expect(screen.getByAltText("workflow")).toHaveAttribute(
      "src",
      "https://cdn.example/description.png",
    );
    expect(screen.getByAltText("task.png")).toHaveAttribute(
      "src",
      "https://cdn.example/task.png",
    );
    expect(
      screen.getByText("Here is the latest screenshot."),
    ).toBeInTheDocument();
    expect(screen.getByAltText("activity.png")).toHaveAttribute(
      "src",
      "https://cdn.example/activity.png",
    );

    fireEvent.click(screen.getByRole("button", { name: "View thread (1)" }));

    expect(screen.getByText("Confirmed in ClickUp.")).toBeInTheDocument();
    expect(screen.getByAltText("reply.png")).toHaveAttribute(
      "src",
      "https://cdn.example/reply.png",
    );
  });

  it("removes line selection while keeping ClickUp content selectable for excerpts", async () => {
    getConversationTicketMock.mockResolvedValue(
      binding({
        ticketRef: { provider: "clickup", id: "task-123", key: "TASK-123" },
        title: "Ship selection snapshots",
      }),
    );
    getTicketDetailMock.mockResolvedValue(
      detail({
        ref: { provider: "clickup", id: "task-123", key: "TASK-123" },
        title: "Ship selection snapshots",
        updatedAt: "2026-07-16T08:00:00Z",
        descriptionMarkdown: "## Scope\n\nKeep the snapshot frozen.",
        descriptionText: "Keep the snapshot frozen.",
        comments: [],
        attachments: [],
      }),
    );
    const onAddExcerpt = vi.fn();
    renderPanel({ onAddExcerpt });

    expect(
      await screen.findByRole("heading", { name: "TASK-123" }),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Select ticket lines" }),
    ).not.toBeInTheDocument();

    const description = await screen.findByText("Keep the snapshot frozen.");
    expect(
      description.closest("[data-artifact-selectable-region='true']"),
    ).not.toBeNull();
    mockSelection(description.firstChild!, "Keep the snapshot frozen.");
    fireEvent.pointerUp(description);
    fireEvent.click(
      screen.getByRole("button", { name: "Add selection to conversation" }),
    );

    expect(onAddExcerpt).toHaveBeenCalledWith({
      sourceKind: "task",
      sourceId: "task-123",
      sourceLabel: "ClickUp task",
      title: "Ship selection snapshots",
      url: "https://app.clickup.com/t/task-123",
      revision: "2026-07-16T08:00:00Z",
      excerpt: "Keep the snapshot frozen.",
    });
  });

  it("does not show loaded-empty states while ticket detail is loading", async () => {
    getTicketDetailMock.mockImplementation(() => new Promise(() => {}));

    renderPanel();

    expect(await screen.findByText("Loading ClickUp task")).toBeInTheDocument();
    expect(
      screen.queryByText("No description provided."),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("No comments yet.")).not.toBeInTheDocument();
  });

  it("keeps the linked summary visible and retries a failed detail request", async () => {
    getTicketDetailMock
      .mockRejectedValueOnce(new Error("ClickUp unavailable"))
      .mockResolvedValueOnce(detail());

    renderPanel();

    expect(
      await screen.findByText("Could not load the ClickUp task"),
    ).toBeInTheDocument();
    expect(screen.getByText("CU-123")).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "Refresh ClickUp task" }),
    );

    await waitFor(() => expect(getTicketDetailMock).toHaveBeenCalledTimes(2));
    expect(
      await screen.findByText("Here is the latest screenshot."),
    ).toBeInTheDocument();
  });
});

function mockSelection(node: Node, text: string) {
  const range = {
    cloneContents: () => document.createDocumentFragment(),
    getBoundingClientRect: () => ({
      bottom: 80,
      height: 20,
      left: 40,
      right: 180,
      top: 60,
      width: 140,
      x: 40,
      y: 60,
      toJSON: () => ({}),
    }),
  } as unknown as Range;
  vi.spyOn(window, "getSelection").mockReturnValue({
    anchorNode: node,
    focusNode: node,
    isCollapsed: false,
    rangeCount: 1,
    getRangeAt: () => range,
    removeAllRanges: vi.fn(),
    toString: () => text,
  } as unknown as Selection);
}
