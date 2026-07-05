import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createTestQueryClient } from "@/test/store-utils";
import { AgentPlanStartPanel } from "./AgentPlanStartPanel";

const {
  copyAgentConversationPlanMock,
  confirmMock,
  getAtVersionMock,
  getVersionHistoryMock,
  importAgentConversationPlanMarkdownMock,
  useAgentComposerPlanReferencesMock,
  useFileDropMock,
  fileDropState,
} = vi.hoisted(() => ({
  copyAgentConversationPlanMock: vi.fn(),
  confirmMock: vi.fn(),
  getAtVersionMock: vi.fn(),
  getVersionHistoryMock: vi.fn(),
  importAgentConversationPlanMarkdownMock: vi.fn(),
  useAgentComposerPlanReferencesMock: vi.fn(),
  useFileDropMock: vi.fn(),
  fileDropState: {
    config: null as unknown,
    result: {
      isDragging: false,
      dropProps: {
        onDragEnter: vi.fn(),
        onDragOver: vi.fn(),
        onDragLeave: vi.fn(),
        onDrop: vi.fn(),
      },
      error: null,
      clearError: vi.fn(),
    },
  },
}));

vi.mock("@/hooks/useAgentComposerResources", () => ({
  useAgentComposerPlanReferences: (...args: unknown[]) =>
    useAgentComposerPlanReferencesMock(...args),
}));

vi.mock("@/api/artifact", () => ({
  artifactApi: {
    getAtVersion: (...args: unknown[]) => getAtVersionMock(...args),
    getVersionHistory: (...args: unknown[]) => getVersionHistoryMock(...args),
  },
}));

vi.mock("@/api/chat", () => ({
  chatApi: {
    copyAgentConversationPlan: (...args: unknown[]) =>
      copyAgentConversationPlanMock(...args),
    importAgentConversationPlanMarkdown: (...args: unknown[]) =>
      importAgentConversationPlanMarkdownMock(...args),
  },
}));

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: () => ({
    confirm: confirmMock,
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  }),
}));

vi.mock("@/hooks/useFileDrop", () => ({
  useFileDrop: (config: unknown) => {
    fileDropState.config = config;
    return useFileDropMock(config);
  },
}));

const projectPlans = [
  {
    sessionId: "source-session-1",
    artifactId: "source-plan-1",
    title: "Existing rollout plan",
    status: "approved" as const,
    artifactVersion: 2,
    updatedAt: "2026-01-24T10:00:00Z",
    approvedAt: "2026-01-24T10:05:00Z",
  },
];

function renderPanel(props: Partial<Parameters<typeof AgentPlanStartPanel>[0]> = {}) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <AgentPlanStartPanel {...props} />
    </QueryClientProvider>,
  );
}

describe("AgentPlanStartPanel", () => {
  beforeEach(() => {
    useAgentComposerPlanReferencesMock.mockReturnValue({
      data: { plans: projectPlans, truncated: false },
      isLoading: false,
      isFetching: false,
      isError: false,
      error: null,
    });
    getVersionHistoryMock.mockResolvedValue([
      {
        id: "source-plan-1",
        version: 2,
        name: "Existing rollout plan",
        created_at: "2026-01-24T10:00:00Z",
      },
      {
        id: "source-plan-1",
        version: 1,
        name: "Existing rollout plan",
        created_at: "2026-01-23T10:00:00Z",
      },
    ]);
    getAtVersionMock.mockImplementation(
      async (_artifactId: string, version: number) => ({
        id: "source-plan-1",
        type: "specification",
        name: "Existing rollout plan",
        content: {
          type: "inline",
          text: version === 1 ? "# First draft" : "# Latest draft",
        },
        metadata: {
          createdAt: "2026-01-24T10:00:00Z",
          createdBy: "orchestrator",
          version,
        },
        derivedFrom: [],
        bucketId: "prd-library",
      }),
    );
    copyAgentConversationPlanMock.mockResolvedValue({
      conversationId: "conversation-1",
      projectId: "project-1",
      planningSessionId: "planning-session-1",
      planArtifactId: "target-plan-1",
      planArtifactVersion: 1,
      sourceArtifactId: "source-plan-1",
      sourceVersion: 1,
      workspace: {},
    });
    importAgentConversationPlanMarkdownMock.mockResolvedValue({
      conversationId: "conversation-1",
      projectId: "project-1",
      planningSessionId: "planning-session-1",
      planArtifactId: "target-plan-2",
      planArtifactVersion: 1,
      sourceArtifactId: null,
      sourceVersion: null,
      workspace: {},
    });
    confirmMock.mockImplementation(async (options) => {
      await options.onConfirm?.();
      return true;
    });
    fileDropState.config = null;
    useFileDropMock.mockReturnValue(fileDropState.result);
  });

  it("renders the lightweight search and import shells", () => {
    renderPanel();

    expect(screen.getByTestId("agent-plan-start-panel")).toBeInTheDocument();
    expect(
      screen.getByRole("searchbox", { name: "Search project plans" }),
    ).toBeDisabled();
    expect(screen.getByText("Import markdown")).toBeInTheDocument();
    expect(screen.getByTestId("agent-plan-start-status-idle")).toHaveTextContent(
      "No plan selected",
    );
  });

  it("renders loading, error, and pending states", () => {
    const queryClient = createTestQueryClient();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <AgentPlanStartPanel status="loading" />
      </QueryClientProvider>,
    );

    expect(screen.getByTestId("agent-plan-start-status-loading")).toHaveTextContent(
      "Loading plans...",
    );

    rerender(
      <QueryClientProvider client={queryClient}>
        <AgentPlanStartPanel
          status="error"
          errorMessage="Unable to prepare plan setup."
        />
      </QueryClientProvider>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Unable to prepare plan setup.",
    );

    rerender(
      <QueryClientProvider client={queryClient}>
        <AgentPlanStartPanel status="pending" />
      </QueryClientProvider>,
    );
    expect(screen.getByTestId("agent-plan-start-status-pending")).toHaveTextContent(
      "Preparing draft plan...",
    );
  });

  it("renders search loading, empty, and error states", () => {
    useAgentComposerPlanReferencesMock.mockReturnValueOnce({
      data: { plans: [], truncated: false },
      isLoading: true,
      isFetching: true,
      isError: false,
      error: null,
    });
    const { rerender } = renderPanel({
      projectId: "project-1",
      conversationId: "conversation-1",
    });

    expect(screen.getByText("Loading project plans...")).toBeInTheDocument();

    useAgentComposerPlanReferencesMock.mockReturnValueOnce({
      data: { plans: [], truncated: false },
      isLoading: false,
      isFetching: false,
      isError: false,
      error: null,
    });
    rerender(
      <QueryClientProvider client={createTestQueryClient()}>
        <AgentPlanStartPanel
          projectId="project-1"
          conversationId="conversation-1"
        />
      </QueryClientProvider>,
    );
    expect(screen.getByText("No plans found")).toBeInTheDocument();

    useAgentComposerPlanReferencesMock.mockReturnValueOnce({
      data: { plans: [], truncated: false },
      isLoading: false,
      isFetching: false,
      isError: true,
      error: new Error("search failed"),
    });
    rerender(
      <QueryClientProvider client={createTestQueryClient()}>
        <AgentPlanStartPanel
          projectId="project-1"
          conversationId="conversation-1"
        />
      </QueryClientProvider>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Plan search failed");
  });

  it("searches project plan references, previews latest by default, and copies the selected version", async () => {
    const onDraftCreated = vi.fn();
    const user = userEvent.setup();

    renderPanel({
      projectId: "project-1",
      conversationId: "conversation-1",
      onDraftCreated,
    });

    expect(useAgentComposerPlanReferencesMock).toHaveBeenCalledWith({
      projectId: "project-1",
      query: "",
      enabled: true,
    });
    await user.click(
      await screen.findByRole("button", {
        name: /select plan existing rollout plan/i,
      }),
    );

    await waitFor(() =>
      expect(getAtVersionMock).toHaveBeenCalledWith("source-plan-1", 2),
    );
    expect(await screen.findByText("# Latest draft")).toBeInTheDocument();

    await user.selectOptions(
      screen.getByLabelText("Preview plan version"),
      "1",
    );
    await waitFor(() =>
      expect(getAtVersionMock).toHaveBeenCalledWith("source-plan-1", 1),
    );
    expect(await screen.findByText("# First draft")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Copy plan" }));

    expect(confirmMock).toHaveBeenCalledWith(
      expect.objectContaining({
        title: "Copy plan?",
        confirmText: "Copy plan",
        pendingText: "Copying...",
      }),
    );
    expect(copyAgentConversationPlanMock).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      sourceSessionId: "source-session-1",
      sourceArtifactId: "source-plan-1",
      sourceVersion: 1,
    });
    await waitFor(() =>
      expect(onDraftCreated).toHaveBeenCalledWith(
        expect.objectContaining({
          planningSessionId: "planning-session-1",
          planArtifactId: "target-plan-1",
        }),
      ),
    );
  });

  it("imports dropped markdown through the frontend-read content path", async () => {
    const onDraftCreated = vi.fn();

    renderPanel({
      projectId: "project-1",
      conversationId: "conversation-1",
      onDraftCreated,
    });

    expect(fileDropState.config).toMatchObject({
      acceptedExtensions: [".md"],
      enabled: true,
    });

    await act(async () => {
      await (fileDropState.config as {
        onFileDrop: (file: File, content: string) => Promise<void>;
      }).onFileDrop(
        new File(["# Imported"], "draft_plan.md", { type: "text/markdown" }),
        "# Imported",
      );
    });

    expect(importAgentConversationPlanMarkdownMock).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      title: "draft plan",
      content: "# Imported",
    });
    await waitFor(() =>
      expect(onDraftCreated).toHaveBeenCalledWith(
        expect.objectContaining({
          planningSessionId: "planning-session-1",
          planArtifactId: "target-plan-2",
        }),
      ),
    );
  });

  it("shows import and file validation errors without calling the success callback", async () => {
    const onDraftCreated = vi.fn();
    importAgentConversationPlanMarkdownMock.mockRejectedValueOnce(
      new Error("Backend rejected stale workspace"),
    );

    renderPanel({
      projectId: "project-1",
      conversationId: "conversation-1",
      onDraftCreated,
    });

    await act(async () => {
      await (fileDropState.config as {
        onFileDrop: (file: File, content: string) => Promise<void>;
      }).onFileDrop(
        new File(["# Imported"], "draft_plan.md", { type: "text/markdown" }),
        "# Imported",
      );
    });

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Backend rejected stale workspace",
    );
    expect(onDraftCreated).not.toHaveBeenCalled();

    await act(async () => {
      (fileDropState.config as {
        onError: (error: { type: string; message: string }) => void;
      }).onError({ type: "invalid_type", message: "Only .md files are accepted" });
    });

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Only .md files are accepted",
    );
  });
});
