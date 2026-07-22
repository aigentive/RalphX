import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  ticketingApi,
  type ConversationTicket,
  type TicketDetail,
} from "@/api/ticketing";
import { TooltipProvider } from "@/components/ui/tooltip";

import { AgentsClickUpIssuePanel } from "./AgentsClickUpIssuePanel";

vi.mock("@/api/ticketing", async () => {
  const actual = await vi.importActual<typeof import("@/api/ticketing")>(
    "@/api/ticketing",
  );
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

function binding(): ConversationTicket {
  return {
    ticketRef: { provider: "clickup", id: "task-123", key: "CU-123" },
    projectId: "project-1",
    title: "Restore rich ClickUp details",
    url: "https://app.clickup.com/t/task-123",
  };
}

function detail(): TicketDetail {
  return {
    ref: { provider: "clickup", id: "task-123", key: "CU-123" },
    title: "Restore rich ClickUp details",
    state: { id: "in-progress", name: "In Progress", category: "in_progress" },
    project: "RalphX",
    assignee: null,
    reporter: { id: "user-1", name: "Alex" },
    labels: ["frontend"],
    priority: "High",
    updatedAt: "2026-07-20T12:00:00Z",
    url: "https://app.clickup.com/t/task-123",
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
  };
}

function renderPanel() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>{children}</TooltipProvider>
    </QueryClientProvider>
  );

  render(<AgentsClickUpIssuePanel conversationId="conversation-1" />, { wrapper });
}

describe("AgentsClickUpIssuePanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getConversationTicketMock.mockResolvedValue(binding());
    getTicketDetailMock.mockResolvedValue(detail());
  });

  it("renders ClickUp description images, task attachments, comments, replies, and comment attachments", async () => {
    renderPanel();

    expect(await screen.findByRole("heading", { name: "Expected" })).toBeInTheDocument();
    expect(screen.getByAltText("workflow")).toHaveAttribute(
      "src",
      "https://cdn.example/description.png",
    );
    expect(screen.getByAltText("task.png")).toHaveAttribute(
      "src",
      "https://cdn.example/task.png",
    );
    expect(screen.getByText("Here is the latest screenshot.")).toBeInTheDocument();
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

  it("does not show loaded-empty states while ticket detail is loading", async () => {
    getTicketDetailMock.mockImplementation(() => new Promise(() => {}));

    renderPanel();

    expect(await screen.findByText("Loading ClickUp task")).toBeInTheDocument();
    expect(screen.queryByText("No description provided.")).not.toBeInTheDocument();
    expect(screen.queryByText("No comments yet.")).not.toBeInTheDocument();
  });

  it("keeps the linked summary visible and retries a failed detail request", async () => {
    getTicketDetailMock
      .mockRejectedValueOnce(new Error("ClickUp unavailable"))
      .mockResolvedValueOnce(detail());

    renderPanel();

    expect(await screen.findByText("Could not load the ClickUp task")).toBeInTheDocument();
    expect(screen.getByText("CU-123")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Refresh ClickUp task" }));

    await waitFor(() => expect(getTicketDetailMock).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("Here is the latest screenshot.")).toBeInTheDocument();
  });
});
