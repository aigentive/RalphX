import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { projectSkillsApi } from "@/api/project-skills";

import { ConversationSkillsPanel } from "./ConversationSkillsPanel";

vi.mock("@/api/project-skills", () => ({
  projectSkillsApi: {
    listForConversation: vi.fn(),
    processConversation: vi.fn(),
  },
}));

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    info: vi.fn(),
    success: vi.fn(),
  },
}));

function renderWithClient(element: React.ReactElement) {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={client}>
      {element}
    </QueryClientProvider>,
  );
}

describe("ConversationSkillsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders conversation-scoped generated and used skills", async () => {
    vi.mocked(projectSkillsApi.listForConversation).mockResolvedValue([
      {
        skill: {
          id: "skill-1",
          projectId: "project-1",
          title: "Check reviewer feedback",
          bucket: "review",
          stage: "review",
          status: "approved",
          pinned: false,
          archived: false,
          scopePaths: [],
          compactGuidance: "Check recurring review feedback before approval.",
          bodyMarkdown: "Detailed guidance",
          predictedEffect: "Reduces repeated review misses.",
          provenance: { conversation_id: "conversation-1" },
          companionOfSkillId: null,
          createdAt: "2026-06-15T10:00:00Z",
          updatedAt: "2026-06-15T10:00:00Z",
        },
        generatedByConversation: true,
        usedByConversation: true,
        usageCount: 2,
      },
    ]);

    renderWithClient(
      <ConversationSkillsPanel
        projectId="project-1"
        conversationId="conversation-1"
      />,
    );

    expect(await screen.findByText("Check reviewer feedback")).toBeInTheDocument();
    expect(screen.getByText("Generated here")).toBeInTheDocument();
    expect(screen.getByText("Used 2 times")).toBeInTheDocument();
    expect(projectSkillsApi.listForConversation).toHaveBeenCalledWith({
      projectId: "project-1",
      conversationId: "conversation-1",
    });
  });

  it("processes the current conversation when refresh is pressed", async () => {
    const user = userEvent.setup();
    vi.mocked(projectSkillsApi.listForConversation).mockResolvedValue([]);
    vi.mocked(projectSkillsApi.processConversation).mockResolvedValue({
      stagedSkills: [],
      skippedExisting: 1,
      messageCount: 3,
    });

    renderWithClient(
      <ConversationSkillsPanel
        projectId="project-1"
        conversationId="conversation-1"
      />,
    );

    await user.click(screen.getByRole("button", { name: /process conversation skills/i }));

    expect(projectSkillsApi.processConversation).toHaveBeenCalledWith({
      projectId: "project-1",
      conversationId: "conversation-1",
    });
  });
});
