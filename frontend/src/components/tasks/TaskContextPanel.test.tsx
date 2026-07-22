/**
 * Tests for TaskContextPanel component
 * Verifies context display, collapsible sections, loading/error states
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TaskContextPanel } from "./TaskContextPanel";
import { taskContextApi } from "@/api/task-context";
import type { TaskContext } from "@/types/task-context";
import userEvent from "@testing-library/user-event";
import { useTaskChatUsageStats } from "@/hooks/useTaskChatUsageStats";

// Mock the API
vi.mock("@/api/task-context", () => ({
  taskContextApi: {
    getTaskContext: vi.fn(),
  },
}));

vi.mock("@/hooks/useTaskChatUsageStats", () => ({
  useTaskChatUsageStats: vi.fn(),
}));

const mockTaskContextApi = taskContextApi as {
  getTaskContext: ReturnType<typeof vi.fn>;
};
const mockUseTaskChatUsageStats = useTaskChatUsageStats as ReturnType<typeof vi.fn>;

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

const mockTask = {
  id: "task-1",
  projectId: "proj-1",
  category: "feature",
  title: "Implement WebSocket server",
  description: "Add WebSocket support",
  priority: 1,
  internalStatus: "backlog" as const,
  needsReviewPoint: false,
  sourceProposalId: "proposal-1",
  planArtifactId: "artifact-1",
  createdAt: "2026-01-26T00:00:00Z",
  updatedAt: "2026-01-26T00:00:00Z",
  startedAt: null,
  completedAt: null,
};

const mockTaskContext: TaskContext = {
  task: mockTask,
  sourceProposal: {
    id: "proposal-1",
    title: "Add Real-Time Communication",
    description: "Implement WebSocket server for real-time updates",
    acceptanceCriteria: [
      "WebSocket server starts successfully",
      "Clients can connect and disconnect",
      "Messages are broadcast to all clients",
    ],
    implementationNotes: "Use tokio-tungstenite library",
    planVersionAtCreation: 3,
  },
  planArtifact: {
    id: "artifact-1",
    title: "WebSocket Implementation Plan",
    artifactType: "specification",
    currentVersion: 5,
    contentPreview:
      "# WebSocket Server Implementation\n\n## Architecture\nUse tokio-tungstenite for async WebSocket handling...",
  },
  relatedArtifacts: [
    {
      id: "artifact-2",
      title: "Real-Time Architecture Research",
      artifactType: "research",
      currentVersion: 1,
      contentPreview: "Research on WebSocket vs SSE vs polling...",
    },
    {
      id: "artifact-3",
      title: "Event System Design",
      artifactType: "design_doc",
      currentVersion: 2,
      contentPreview: "Event-driven architecture for real-time updates...",
    },
  ],
  contextHints: [
    "Review the implementation plan for architectural decisions",
    "Check related research for performance considerations",
  ],
};

describe("TaskContextPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUseTaskChatUsageStats.mockReturnValue({ data: undefined });
  });

  describe("Loading State", () => {
    it("shows loading skeleton while fetching context", () => {
      mockTaskContextApi.getTaskContext.mockReturnValue(
        new Promise(() => {}) // Never resolves
      );

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      // Check for skeleton loading animation
      const skeletons = document.querySelectorAll(".animate-pulse");
      expect(skeletons.length).toBeGreaterThan(0);
    });
  });

  describe("Error State", () => {
    it("displays error message when fetch fails", async () => {
      const error = new Error("Failed to fetch task context");
      mockTaskContextApi.getTaskContext.mockRejectedValue(error);

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("Failed to load task context")).toBeInTheDocument();
        expect(screen.getByText("Failed to fetch task context")).toBeInTheDocument();
      });
    });
  });

  describe("Empty State", () => {
    it("shows empty state when no context available", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue({
        task: mockTask,
        sourceProposal: null,
        planArtifact: null,
        relatedArtifacts: [],
        contextHints: [],
      });

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("No context available for this task")).toBeInTheDocument();
      });
    });
  });

  describe("Source Proposal Section", () => {
    it("displays source proposal when available", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("Source Proposal")).toBeInTheDocument();
        expect(screen.getByText("Add Real-Time Communication")).toBeInTheDocument();
        expect(
          screen.getByText("Implement WebSocket server for real-time updates")
        ).toBeInTheDocument();
      });
    });

    it("displays acceptance criteria", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("Acceptance Criteria")).toBeInTheDocument();
        expect(screen.getByText("WebSocket server starts successfully")).toBeInTheDocument();
        expect(screen.getByText("Clients can connect and disconnect")).toBeInTheDocument();
        expect(screen.getByText("Messages are broadcast to all clients")).toBeInTheDocument();
      });
    });

    it("displays implementation notes", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("Implementation Notes")).toBeInTheDocument();
        expect(screen.getByText("Use tokio-tungstenite library")).toBeInTheDocument();
      });
    });

    it("displays plan version at creation", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("Plan version at creation:")).toBeInTheDocument();
        expect(screen.getByText("3")).toBeInTheDocument();
      });
    });

    it("is collapsible", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);
      const user = userEvent.setup();

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("Add Real-Time Communication")).toBeInTheDocument();
      });

      // Find the collapse button within the Source Proposal section
      const proposalSection = screen.getByText("Source Proposal").closest("div");
      const collapseButton = proposalSection?.querySelector("button");

      if (collapseButton) {
        await user.click(collapseButton);

        // Content should be hidden (removed from DOM)
        await waitFor(() => {
          expect(screen.queryByText("Add Real-Time Communication")).not.toBeInTheDocument();
        });
      }
    });
  });

  describe("Plan Artifact Section", () => {
    it("displays plan artifact when available", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("Implementation Plan")).toBeInTheDocument();
        expect(screen.getByText("WebSocket Implementation Plan")).toBeInTheDocument();
        expect(screen.getByText("v5")).toBeInTheDocument();
      });
    });

    it("displays content preview", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(
          screen.getByText(/# WebSocket Server Implementation/)
        ).toBeInTheDocument();
      });
    });

    it("calls onViewArtifact when View Full Plan clicked", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);
      const onViewArtifact = vi.fn();
      const user = userEvent.setup();

      render(
        <TaskContextPanel taskId="task-1" onViewArtifact={onViewArtifact} />,
        { wrapper: createWrapper() }
      );

      await waitFor(() => {
        expect(screen.getByText("View Full Plan")).toBeInTheDocument();
      });

      await user.click(screen.getByText("View Full Plan"));

      expect(onViewArtifact).toHaveBeenCalledWith("artifact-1");
    });

    it("is collapsible", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);
      const user = userEvent.setup();

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("WebSocket Implementation Plan")).toBeInTheDocument();
      });

      // Find the collapse button within the Implementation Plan section
      const planSection = screen.getByText("Implementation Plan").closest("div");
      const collapseButton = planSection?.querySelector("button");

      if (collapseButton) {
        await user.click(collapseButton);

        // Content should be hidden
        await waitFor(() => {
          expect(screen.queryByText("WebSocket Implementation Plan")).not.toBeInTheDocument();
        });
      }
    });
  });

  describe("Related Artifacts Section", () => {
    it("displays related artifacts when available", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("Related Artifacts")).toBeInTheDocument();
        expect(screen.getByText("(2)")).toBeInTheDocument();
        expect(screen.getByText("Real-Time Architecture Research")).toBeInTheDocument();
        expect(screen.getByText("Event System Design")).toBeInTheDocument();
      });
    });

    it("displays artifact type and version", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("v1")).toBeInTheDocument();
        expect(screen.getByText("v2")).toBeInTheDocument();
      });
    });

    it("is collapsible", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);
      const user = userEvent.setup();

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("Real-Time Architecture Research")).toBeInTheDocument();
      });

      // Find the collapse button within the Related Artifacts section
      const artifactsSection = screen.getByText("Related Artifacts").closest("div");
      const collapseButton = artifactsSection?.querySelector("button");

      if (collapseButton) {
        await user.click(collapseButton);

        // Content should be hidden
        await waitFor(() => {
          expect(screen.queryByText("Real-Time Architecture Research")).not.toBeInTheDocument();
        });
      }
    });

    it("hides section when no related artifacts", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue({
        ...mockTaskContext,
        relatedArtifacts: [],
      });

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByTestId("task-context-panel")).toBeInTheDocument();
      });

      expect(screen.queryByText("Related Artifacts")).not.toBeInTheDocument();
    });
  });

  describe("Context Hints Section", () => {
    it("displays context hints when available", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("Context Hints")).toBeInTheDocument();
        expect(
          screen.getByText("Review the implementation plan for architectural decisions")
        ).toBeInTheDocument();
        expect(
          screen.getByText("Check related research for performance considerations")
        ).toBeInTheDocument();
      });
    });

    it("hides section when no hints", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue({
        ...mockTaskContext,
        contextHints: [],
      });

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByTestId("task-context-panel")).toBeInTheDocument();
      });

      expect(screen.queryByText("Context Hints")).not.toBeInTheDocument();
    });
  });

  describe("Integration", () => {
    it("shows task usage card when scoped usage stats are available", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);
      mockUseTaskChatUsageStats.mockReturnValue({
        data: {
          scopeType: "task",
          scopeId: "task-1",
          conversationCount: 2,
          messageUsageTotals: {
            inputTokens: 300,
            outputTokens: 90,
            cacheCreationTokens: 20,
            cacheReadTokens: 30,
            processedTokens: 390,
            estimatedUsd: 0.52,
          },
          runUsageTotals: {
            inputTokens: 300,
            outputTokens: 90,
            cacheCreationTokens: 20,
            cacheReadTokens: 30,
            processedTokens: 390,
            estimatedUsd: 0.52,
          },
          effectiveUsageTotals: {
            inputTokens: 300,
            outputTokens: 90,
            cacheCreationTokens: 20,
            cacheReadTokens: 30,
            processedTokens: 390,
            estimatedUsd: 0.52,
          },
          usageCoverage: {
            providerMessageCount: 4,
            providerMessagesWithUsage: 4,
            runCount: 2,
            runsWithUsage: 2,
            effectiveRunConversationCount: 0,
            effectiveMessageConversationCount: 2,
            legacyEstimatedSampleCount: 0,
            fallbackEstimatedSampleCount: 0,
            uncountedSampleCount: 0,
            effectiveTotalsSource: "messages",
          },
          attributionCoverage: {
            providerMessageCount: 4,
            providerMessagesWithAttribution: 4,
            runCount: 2,
            runsWithAttribution: 2,
          },
          byContextType: [],
          byHarness: [
            {
              key: "codex",
              count: 2,
              usage: {
                inputTokens: 300,
                outputTokens: 90,
                cacheCreationTokens: 20,
                cacheReadTokens: 30,
                processedTokens: 390,
                estimatedUsd: 0.52,
              },
            },
          ],
          byUpstreamProvider: [],
          byModel: [
            {
              key: "gpt-5.4",
              count: 2,
              usage: {
                inputTokens: 300,
                outputTokens: 90,
                cacheCreationTokens: 20,
                cacheReadTokens: 30,
                processedTokens: 390,
                estimatedUsd: 0.52,
              },
            },
          ],
          byEffort: [],
        },
      });

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("AI Usage")).toBeInTheDocument();
        expect(screen.getByText("Harness: codex")).toBeInTheDocument();
      });
    });

    it("displays all sections when full context available", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue(mockTaskContext);

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("Source Proposal")).toBeInTheDocument();
        expect(screen.getByText("Implementation Plan")).toBeInTheDocument();
        expect(screen.getByText("Related Artifacts")).toBeInTheDocument();
        expect(screen.getByText("Context Hints")).toBeInTheDocument();
      });
    });

    it("only displays available sections", async () => {
      mockTaskContextApi.getTaskContext.mockResolvedValue({
        task: mockTask,
        sourceProposal: mockTaskContext.sourceProposal,
        planArtifact: null,
        relatedArtifacts: [],
        contextHints: [],
      });

      render(<TaskContextPanel taskId="task-1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("Source Proposal")).toBeInTheDocument();
      });

      expect(screen.queryByText("Implementation Plan")).not.toBeInTheDocument();
      expect(screen.queryByText("Related Artifacts")).not.toBeInTheDocument();
      expect(screen.queryByText("Context Hints")).not.toBeInTheDocument();
    });
  });
});
