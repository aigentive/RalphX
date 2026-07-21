import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useConversationTicket, useTicketDetail } from "@/hooks/useTicketing";
import { ArtifactSelectionProvider } from "./artifact-selection/ArtifactSelectionProvider";
import { AgentsClickUpIssuePanel } from "./AgentsClickUpIssuePanel";

vi.mock("@/hooks/useTicketing", () => ({
  useConversationTicket: vi.fn(),
  useTicketDetail: vi.fn(),
}));

describe("AgentsClickUpIssuePanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("removes line selection while keeping ClickUp content selectable for excerpts", async () => {
    const onAddExcerpt = vi.fn();
    render(
      <ArtifactSelectionProvider enabled onAddExcerpt={onAddExcerpt}>
        <AgentsClickUpIssuePanel conversationId="conversation-1" />
      </ArtifactSelectionProvider>,
    );

    expect(screen.getByRole("heading", { name: "TASK-123" })).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Select ticket lines" }),
    ).not.toBeInTheDocument();

    const description = screen.getByText("Keep the snapshot frozen.");
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
