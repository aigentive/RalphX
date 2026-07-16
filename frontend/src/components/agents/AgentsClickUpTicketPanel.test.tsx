import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useConversationTicket, useTicketDetail } from "@/hooks/useTicketing";
import { TooltipProvider } from "@/components/ui/tooltip";

import { AgentsClickUpTicketPanel } from "./AgentsClickUpTicketPanel";

vi.mock("@/hooks/useTicketing", () => ({
  useConversationTicket: vi.fn(),
  useTicketDetail: vi.fn(),
}));

vi.mock("@/components/ticketing/useAfterPaint", () => ({
  useAfterPaint: () => true,
}));

describe("AgentsClickUpTicketPanel", () => {
  beforeEach(() => {
    vi.mocked(useConversationTicket).mockReturnValue({
      data: {
        ticketRef: { provider: "clickup", id: "8689abc", key: "CU-42" },
        projectId: "project-1",
        title: "ClickUp context bug",
        url: "https://app.clickup.com/t/8689abc",
      },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof useConversationTicket>);
    vi.mocked(useTicketDetail).mockReturnValue({
      data: {
        ref: { provider: "clickup", id: "8689abc", key: "CU-42" },
        title: "ClickUp context bug",
        state: { id: "in-progress", name: "In Progress", category: "in_progress" },
        assignees: [{ id: "user-1", name: "Sam" }],
        watchers: [],
        labels: ["backend", "bug"],
        sprints: [],
        updatedAt: "2026-07-15T12:00:00Z",
        url: "https://app.clickup.com/t/8689abc",
        associationCount: 1,
        openPrCount: 0,
        descriptionMarkdown: "The agent needs the **ClickUp task body**.",
        comments: [],
        attachments: [],
        transitions: [],
      },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof useTicketDetail>);
  });

  it("renders the linked ClickUp task details", () => {
    render(
      <TooltipProvider>
        <AgentsClickUpTicketPanel
          conversationId="conversation-1"
          projectId="project-1"
        />
      </TooltipProvider>,
    );

    expect(screen.getAllByText("CU-42")).toHaveLength(2);
    expect(screen.getByText("ClickUp context bug")).toBeInTheDocument();
    expect(screen.getByText("In Progress")).toBeInTheDocument();
    expect(screen.getByText(/ClickUp task body/)).toBeInTheDocument();
    expect(screen.getByText("Sam")).toBeInTheDocument();
    expect(screen.getByText("backend")).toBeInTheDocument();
  });
});
