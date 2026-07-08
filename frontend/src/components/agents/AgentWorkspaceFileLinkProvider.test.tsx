import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";

import { chatApi, type WorkspaceOpenTarget } from "@/api/chat";
import { useMessageFileLinkContext } from "@/components/Chat/MessageFileLinkContext";
import { writePreferredWorkspaceOpenTargetId } from "@/lib/workspace-open-targets";

import { conversationWorkspaceFixture as conversationWorkspace } from "./agentsTestFixtures";
import { AgentWorkspaceFileLinkProvider } from "./AgentWorkspaceFileLinkProvider";

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      listWorkspaceOpenTargets: vi.fn(),
      openAgentConversationWorkspacePath: vi.fn(),
    },
  };
});

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
  },
}));

const targets: WorkspaceOpenTarget[] = [
  { id: "cursor", label: "Cursor", kind: "editor" },
  { id: "finder", label: "Finder", kind: "fileManager" },
];

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

function ContextProbe({
  path = "/tmp/ralphx/conversation-1/src/lib.rs",
}: {
  path?: string;
}) {
  const context = useMessageFileLinkContext();
  if (!context) {
    return <div data-testid="file-link-context">none</div>;
  }

  return (
    <div>
      <div data-testid="file-link-context">present</div>
      <div data-testid="workspace-root">{context.workspaceRootPath}</div>
      <div data-testid="preferred-target">{context.preferredTargetId ?? ""}</div>
      <div data-testid="opening-path">{context.openingPath ?? ""}</div>
      <div data-testid="opening-target">{context.openingTargetId ?? ""}</div>
      <div data-testid="target-count">{context.targets.length}</div>
      <button
        type="button"
        onClick={() => context.onOpenPath(path, "finder")}
      >
        Open Finder
      </button>
    </div>
  );
}

function renderProvider({
  children = <ContextProbe />,
  workspace = conversationWorkspace(),
}: {
  children?: ReactNode;
  workspace?: ReturnType<typeof conversationWorkspace> | null;
} = {}) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  return {
    ...render(
      <QueryClientProvider client={queryClient}>
        <AgentWorkspaceFileLinkProvider
          conversationId="conversation-1"
          workspace={workspace}
        >
          {children}
        </AgentWorkspaceFileLinkProvider>
      </QueryClientProvider>,
    ),
    queryClient,
  };
}

describe("AgentWorkspaceFileLinkProvider", () => {
  beforeEach(() => {
    window.localStorage.clear();
    vi.mocked(chatApi.listWorkspaceOpenTargets).mockReset();
    vi.mocked(chatApi.listWorkspaceOpenTargets).mockResolvedValue(targets);
    vi.mocked(chatApi.openAgentConversationWorkspacePath).mockReset();
    vi.mocked(chatApi.openAgentConversationWorkspacePath).mockResolvedValue(undefined);
    vi.mocked(toast.error).mockReset();
  });

  it("provides workspace file link context from cached open targets", async () => {
    renderProvider();

    await waitFor(() => {
      expect(screen.getByTestId("file-link-context")).toHaveTextContent("present");
    });
    expect(screen.getByTestId("workspace-root")).toHaveTextContent(
      "/tmp/ralphx/conversation-1",
    );
    expect(screen.getByTestId("preferred-target")).toHaveTextContent("");
    await waitFor(() => {
      expect(screen.getByTestId("target-count")).toHaveTextContent("2");
    });
  });

  it("provides workspace root context before open targets finish loading", () => {
    const pendingTargets = deferred<WorkspaceOpenTarget[]>();
    vi.mocked(chatApi.listWorkspaceOpenTargets).mockReturnValue(
      pendingTargets.promise,
    );

    renderProvider();

    expect(screen.getByTestId("file-link-context")).toHaveTextContent("present");
    expect(screen.getByTestId("workspace-root")).toHaveTextContent(
      "/tmp/ralphx/conversation-1",
    );
    expect(screen.getByTestId("target-count")).toHaveTextContent("0");
  });

  it("opens a workspace path with the selected target and tracks in-flight state", async () => {
    const user = userEvent.setup();
    const pendingOpen = deferred<void>();
    vi.mocked(chatApi.openAgentConversationWorkspacePath).mockReturnValue(
      pendingOpen.promise,
    );
    renderProvider();

    await waitFor(() => {
      expect(screen.getByTestId("file-link-context")).toHaveTextContent("present");
    });
    await user.click(screen.getByRole("button", { name: "Open Finder" }));

    expect(chatApi.openAgentConversationWorkspacePath).toHaveBeenCalledWith(
      "conversation-1",
      "finder",
      "/tmp/ralphx/conversation-1/src/lib.rs",
    );
    expect(window.localStorage.getItem("ralphx:agents:preferred-workspace-open-target")).toBe(
      "finder",
    );
    expect(screen.getByTestId("opening-path")).toHaveTextContent(
      "/tmp/ralphx/conversation-1/src/lib.rs",
    );
    expect(screen.getByTestId("opening-target")).toHaveTextContent("finder");

    await act(async () => {
      pendingOpen.resolve();
      await pendingOpen.promise;
    });

    await waitFor(() => {
      expect(screen.getByTestId("opening-path")).toHaveTextContent("");
    });
  });

  it("opens workspace paths using the workspace owner conversation id", async () => {
    const user = userEvent.setup();
    renderProvider({
      workspace: conversationWorkspace({
        conversationId: "workspace-conversation",
        linkedPlanBranchId: "plan-branch-1",
      }),
    });

    await waitFor(() => {
      expect(screen.getByTestId("file-link-context")).toHaveTextContent("present");
    });
    await user.click(screen.getByRole("button", { name: "Open Finder" }));

    expect(chatApi.openAgentConversationWorkspacePath).toHaveBeenCalledWith(
      "workspace-conversation",
      "finder",
      "/tmp/ralphx/conversation-1/src/lib.rs",
    );
  });

  it("reports backend open failures with the returned message", async () => {
    const user = userEvent.setup();
    vi.mocked(chatApi.openAgentConversationWorkspacePath).mockRejectedValue(
      "Finder failed",
    );
    renderProvider();

    await waitFor(() => {
      expect(screen.getByTestId("file-link-context")).toHaveTextContent("present");
    });
    await user.click(screen.getByRole("button", { name: "Open Finder" }));

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith("Finder failed");
    });
    expect(screen.getByTestId("opening-path")).toHaveTextContent("");
  });

  it("syncs preferred target changes written by another control", async () => {
    renderProvider();

    await waitFor(() => {
      expect(screen.getByTestId("file-link-context")).toHaveTextContent("present");
    });
    expect(screen.getByTestId("preferred-target")).toHaveTextContent("");

    act(() => {
      writePreferredWorkspaceOpenTargetId("finder");
    });

    await waitFor(() => {
      expect(screen.getByTestId("preferred-target")).toHaveTextContent("finder");
    });
  });

  it("falls back to the first available target when a stored preference is stale", async () => {
    window.localStorage.setItem(
      "ralphx:agents:preferred-workspace-open-target",
      "missing",
    );

    renderProvider();

    await waitFor(() => {
      expect(screen.getByTestId("preferred-target")).toHaveTextContent("cursor");
    });
  });

  it("does not query or provide context without a workspace", () => {
    renderProvider({ workspace: null });

    expect(screen.getByTestId("file-link-context")).toHaveTextContent("none");
    expect(chatApi.listWorkspaceOpenTargets).not.toHaveBeenCalled();
  });
});
