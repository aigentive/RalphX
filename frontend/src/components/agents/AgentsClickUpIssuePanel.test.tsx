import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useConversationTicket, useTicketDetail } from "@/hooks/useTicketing";
import { useArtifactSelectionStore } from "@/stores/artifactSelectionStore";
import { AgentsClickUpIssuePanel } from "./AgentsClickUpIssuePanel";

vi.mock("@/hooks/useTicketing", () => ({
  useConversationTicket: vi.fn(),
  useTicketDetail: vi.fn(),
}));

describe("AgentsClickUpIssuePanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useArtifactSelectionStore.getState().clearAllSelections();
    vi.mocked(useConversationTicket).mockReturnValue({
      data: {
        ticketRef: { provider: "clickup", id: "task-123", key: "TASK-123" },
        projectId: "project-1",
        title: "Ship selection snapshots",
        url: "https://app.clickup.com/t/task-123",
      },
      isLoading: false,
      error: null,
    } as ReturnType<typeof useConversationTicket>);
    vi.mocked(useTicketDetail).mockReturnValue({
      data: {
        ref: { provider: "clickup", id: "task-123", key: "TASK-123" },
        title: "Ship selection snapshots",
        state: { id: "in-progress", name: "In Progress", category: "in_progress" },
        assignees: [],
        watchers: [],
        labels: ["frontend"],
        sprints: [],
        updatedAt: "2026-07-16T08:00:00Z",
        associationCount: 0,
        openPrCount: 0,
        currentUserAssigned: false,
        currentUserWatching: false,
        descriptionMarkdown: "## Scope\n\nKeep the snapshot frozen.",
        comments: [],
        attachments: [],
        transitions: [],
      },
      isLoading: false,
      error: null,
    } as ReturnType<typeof useTicketDetail>);
  });

  it("renders the linked ClickUp task and commits a frozen source line", async () => {
    render(<AgentsClickUpIssuePanel conversationId="conversation-1" />);

    expect(screen.getByRole("heading", { name: "TASK-123" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Select ticket lines" }));
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Line 1: # TASK-123: Ship selection snapshots",
      }),
    );

    expect(
      useArtifactSelectionStore.getState().selections["conversation-1"],
    ).toEqual(
      expect.objectContaining({
        sourceType: "ticket",
        sourceKind: "clickup",
        sourceId: "task-123",
        sourceKey: "TASK-123",
        provider: "clickup",
        sourceRevision: "2026-07-16T08:00:00Z",
        content: "# TASK-123: Ship selection snapshots",
      }),
    );
  });
});
