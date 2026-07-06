import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { FileDropConfig } from "@/hooks/useFileDrop";
import { createTestQueryClient } from "@/test/store-utils";
import { AgentPlanStartPanel } from "./AgentPlanStartPanel";

const {
  useAgentComposerPlanReferencesMock,
  getAtVersionMock,
  getVersionHistoryMock,
  copyAgentConversationPlanMock,
  importAgentConversationPlanMock,
  useFileDropMock,
  toastErrorMock,
  toastSuccessMock,
  fileDropConfig,
  planReferencesState,
} = vi.hoisted(() => ({
  useAgentComposerPlanReferencesMock: vi.fn(),
  getAtVersionMock: vi.fn(),
  getVersionHistoryMock: vi.fn(),
  copyAgentConversationPlanMock: vi.fn(),
  importAgentConversationPlanMock: vi.fn(),
  useFileDropMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  fileDropConfig: { current: null as FileDropConfig | null },
  planReferencesState: {
    current: {
      data: { plans: [], truncated: false },
      isFetching: false,
      isLoading: false,
      isError: false,
      error: null,
    },
  },
}));

vi.mock("@/hooks/useAgentComposerResources", () => ({
  useAgentComposerPlanReferences: (...args: unknown[]) =>
    useAgentComposerPlanReferencesMock(...args),
}));

vi.mock("@/api/artifact", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/artifact")>();
  return {
    ...actual,
    artifactApi: {
      ...actual.artifactApi,
      getAtVersion: (...args: unknown[]) => getAtVersionMock(...args),
      getVersionHistory: (...args: unknown[]) => getVersionHistoryMock(...args),
    },
  };
});

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      copyAgentConversationPlan: (...args: unknown[]) =>
        copyAgentConversationPlanMock(...args),
      importAgentConversationPlan: (...args: unknown[]) =>
        importAgentConversationPlanMock(...args),
    },
  };
});

vi.mock("@/hooks/useFileDrop", () => ({
  useFileDrop: (config: FileDropConfig) => useFileDropMock(config),
}));

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

function renderPanel(onPlanSeeded = vi.fn()) {
  return {
    onPlanSeeded,
    ...render(
      <QueryClientProvider client={createTestQueryClient()}>
        <AgentPlanStartPanel
          conversationId="conversation-1"
          projectId="project-1"
          onPlanSeeded={onPlanSeeded}
        />
      </QueryClientProvider>,
    ),
  };
}

describe("AgentPlanStartPanel", () => {
  beforeEach(() => {
    planReferencesState.current = {
      data: { plans: [], truncated: false },
      isFetching: false,
      isLoading: false,
      isError: false,
      error: null,
    };
    useAgentComposerPlanReferencesMock.mockImplementation(
      () => planReferencesState.current,
    );
    getAtVersionMock.mockResolvedValue(null);
    getVersionHistoryMock.mockResolvedValue([]);
    copyAgentConversationPlanMock.mockResolvedValue({
      sessionId: "target-session-1",
      artifact: {
        id: "target-plan-1",
        type: "specification",
        name: "Copied plan",
        content: { type: "inline", text: "# Copied plan" },
        metadata: {
          createdAt: "2026-04-23T09:00:00Z",
          createdBy: "user",
          version: 1,
        },
        derivedFrom: ["source-v1"],
        bucketId: "prd-library",
        planApproval: { status: "draft" },
      },
      workspace: { conversationId: "conversation-1", mode: "plan" },
      conversation: { id: "conversation-1" },
    });
    importAgentConversationPlanMock.mockResolvedValue({
      sessionId: "target-session-1",
      artifact: {
        id: "imported-plan-1",
        type: "specification",
        name: "Dropped plan",
        content: { type: "inline", text: "# Dropped plan" },
        metadata: {
          createdAt: "2026-04-23T09:00:00Z",
          createdBy: "user",
          version: 1,
        },
        derivedFrom: [],
        bucketId: "prd-library",
        planApproval: { status: "draft" },
      },
      workspace: { conversationId: "conversation-1", mode: "plan" },
      conversation: { id: "conversation-1" },
    });
    useFileDropMock.mockImplementation((config: FileDropConfig) => {
      fileDropConfig.current = config;
      return {
        isDragging: false,
        dropProps: {},
        error: null,
        clearError: vi.fn(),
      };
    });
    toastErrorMock.mockClear();
    toastSuccessMock.mockClear();
    fileDropConfig.current = null;
  });

  it("defers plan search until intent and copies the selected historical version", async () => {
    const user = userEvent.setup();
    planReferencesState.current = {
      data: {
        plans: [
          {
            sessionId: "source-session-1",
            artifactId: "source-plan-latest",
            title: "Checkout flow plan",
            status: "approved",
            artifactVersion: 2,
            updatedAt: "2026-04-23T09:00:00Z",
            approvedAt: "2026-04-23T09:30:00Z",
          },
        ],
        truncated: false,
      },
      isFetching: false,
      isLoading: false,
      isError: false,
      error: null,
    };
    getVersionHistoryMock.mockResolvedValue([
      {
        id: "source-plan-latest",
        version: 2,
        name: "Checkout flow plan",
        created_at: "2026-04-23T09:30:00Z",
      },
      {
        id: "source-plan-v1",
        version: 1,
        name: "Checkout flow plan",
        created_at: "2026-04-23T09:00:00Z",
      },
    ]);
    getAtVersionMock.mockImplementation(async (_artifactId: string, version: number) => ({
      id: version === 1 ? "source-plan-v1" : "source-plan-latest",
      type: "specification",
      name: "Checkout flow plan",
      content: {
        type: "inline",
        text: version === 1 ? "# Checkout v1" : "# Checkout v2",
      },
      metadata: {
        createdAt: "2026-04-23T09:00:00Z",
        createdBy: "orchestrator",
        version,
      },
      derivedFrom: [],
      bucketId: "prd-library",
    }));
    const { onPlanSeeded } = renderPanel();

    expect(useAgentComposerPlanReferencesMock).toHaveBeenLastCalledWith({
      projectId: "project-1",
      query: "",
      enabled: false,
    });

    await user.click(screen.getByLabelText("Search project plans"));
    expect(useAgentComposerPlanReferencesMock).toHaveBeenLastCalledWith({
      projectId: "project-1",
      query: "",
      enabled: true,
    });

    await user.click(screen.getByRole("button", { name: /Checkout flow plan/ }));
    await waitFor(() =>
      expect(getAtVersionMock).toHaveBeenCalledWith("source-plan-latest", 2),
    );
    expect(await screen.findByText("# Checkout v2")).toBeInTheDocument();

    await user.selectOptions(screen.getByLabelText("Plan version"), "1");
    await waitFor(() =>
      expect(getAtVersionMock).toHaveBeenCalledWith("source-plan-latest", 1),
    );
    expect(await screen.findByText("# Checkout v1")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Copy plan" }));

    await waitFor(() =>
      expect(copyAgentConversationPlanMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        sourceSessionId: "source-session-1",
        sourceArtifactId: "source-plan-latest",
        sourceVersion: 1,
      }),
    );
    expect(onPlanSeeded).toHaveBeenCalledWith(
      expect.objectContaining({ sessionId: "target-session-1" }),
    );
  });

  it("imports dropped markdown content through the Agent plan command", async () => {
    const { onPlanSeeded } = renderPanel();

    expect(fileDropConfig.current).toMatchObject({
      acceptedExtensions: [".md"],
      enabled: true,
    });

    await fileDropConfig.current?.onFileDrop(
      new File(["# Dropped plan"], "dropped_plan.md", {
        type: "text/markdown",
      }),
      "# Dropped plan",
    );

    await waitFor(() =>
      expect(importAgentConversationPlanMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        title: "dropped plan",
        content: "# Dropped plan",
      }),
    );
    expect(onPlanSeeded).toHaveBeenCalledWith(
      expect.objectContaining({ sessionId: "target-session-1" }),
    );
  });

  it("falls back to selected plan metadata when version history is unavailable", async () => {
    const user = userEvent.setup();
    planReferencesState.current = {
      data: {
        plans: [
          {
            sessionId: "source-session-2",
            artifactId: "source-plan-file",
            title: "File backed plan",
            status: "draft",
            artifactVersion: 3,
            updatedAt: "2026-04-23T10:00:00Z",
            approvedAt: null,
          },
        ],
        truncated: false,
      },
      isFetching: false,
      isLoading: false,
      isError: false,
      error: null,
    };
    getVersionHistoryMock.mockResolvedValue([]);
    getAtVersionMock.mockResolvedValue({
      id: "source-plan-file",
      type: "specification",
      name: "File backed plan",
      content: { type: "file", path: "/tmp/file-backed-plan.md" },
      metadata: {
        createdAt: "2026-04-23T10:00:00Z",
        createdBy: "orchestrator",
        version: 3,
      },
      derivedFrom: [],
      bucketId: "prd-library",
    });

    renderPanel();

    await user.click(screen.getByLabelText("Search project plans"));
    await user.click(screen.getByRole("button", { name: /File backed plan/ }));

    expect(await screen.findByRole("option", { name: "v3" })).toBeInTheDocument();
    expect(
      await screen.findByText("File-backed preview unavailable"),
    ).toBeInTheDocument();
  });

  it("surfaces file drop and import errors", async () => {
    const { onPlanSeeded } = renderPanel();
    importAgentConversationPlanMock.mockRejectedValueOnce(
      new Error("Import failed"),
    );

    fileDropConfig.current?.onError({
      message: "Only Markdown files are supported",
    });

    expect(toastErrorMock).toHaveBeenCalledWith(
      "Only Markdown files are supported",
    );

    fileDropConfig.current?.onFileDrop(
      new File(["# Broken plan"], "broken.md", { type: "text/markdown" }),
      "# Broken plan",
    );

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith("Import failed"),
    );
    expect(onPlanSeeded).not.toHaveBeenCalled();
  });
});
