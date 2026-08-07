import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState, type ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { useUiStore } from "@/stores/uiStore";
import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";
import type { Task } from "@/types/task";
import type { TaskHistoryState } from "@/types/task-history";

import { AgentsTaskDetailOverlay } from "./AgentsTaskDetailOverlay";

const {
  useTasksMock,
  useTaskMutationMock,
  createIdeationSessionMock,
  getTaskMock,
} = vi.hoisted(() => ({
  useTasksMock: vi.fn(),
  useTaskMutationMock: vi.fn(),
  createIdeationSessionMock: vi.fn(),
  getTaskMock: vi.fn(),
}));

vi.mock("@/hooks/useTasks", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/hooks/useTasks")>();
  return {
    ...actual,
    useTasks: (...args: unknown[]) => useTasksMock(...args),
  };
});

vi.mock("@/hooks/useTaskMutation", () => ({
  useTaskMutation: (...args: unknown[]) => useTaskMutationMock(...args),
}));

vi.mock("@/hooks/useIdeation", () => ({
  useCreateIdeationSession: () => ({
    mutateAsync: createIdeationSessionMock,
    isPending: false,
  }),
}));

vi.mock("@/stores/ideationStore", () => ({
  useIdeationStore: (selector: (state: unknown) => unknown) =>
    selector({
      addSession: vi.fn(),
      setActiveSession: vi.fn(),
    }),
}));

vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: {
      get: (...args: unknown[]) => getTaskMock(...args),
    },
  },
}));

vi.mock("./AgentsTaskDetailPanel", () => ({
  AgentsTaskDetailPanel: ({
    viewAsStatus,
    viewConversationId,
  }: {
    viewAsStatus?: string;
    viewConversationId?: string;
  }) => (
    <div
      data-testid="mock-task-detail-panel"
      data-view-status={viewAsStatus ?? ""}
      data-view-conversation-id={viewConversationId ?? ""}
    />
  ),
}));

vi.mock("./TaskEditForm", () => ({
  TaskEditForm: () => <div data-testid="mock-task-edit-form" />,
}));

vi.mock("./StatusDropdown", () => ({
  StatusDropdown: () => <div data-testid="mock-status-dropdown" />,
}));

vi.mock("./AuditTrailDialog", () => ({
  AuditTrailDialog: () => null,
}));

vi.mock("./TaskHistoryDropdown", () => ({
  TaskHistoryDropdown: ({
    onStateSelect,
  }: {
    onStateSelect: (state: TaskHistoryState | null) => void;
  }) => (
    <div data-testid="mock-task-history-dropdown">
      <button
        type="button"
        onClick={() =>
          onStateSelect({
            status: "executing",
            timestamp: "2026-07-07T10:00:00Z",
            conversationId: "exec-conversation",
            contextType: "task_execution",
            attemptIndex: 1,
            hasConversation: true,
          })
        }
      >
        Select execution
      </button>
      <button
        type="button"
        onClick={() =>
          onStateSelect({
            status: "reviewing",
            timestamp: "2026-07-07T10:10:00Z",
            conversationId: "review-conversation",
            contextType: "review",
            attemptIndex: 1,
            hasConversation: true,
          })
        }
      >
        Select review
      </button>
      <button
        type="button"
        onClick={() =>
          onStateSelect({
            status: "merged",
            timestamp: "2026-07-07T10:20:00Z",
            conversationId: "merge-conversation",
            contextType: "merge",
            attemptIndex: 1,
            hasConversation: true,
          })
        }
      >
        Select merge
      </button>
      <button
        type="button"
        onClick={() =>
          onStateSelect({
            status: "reviewing",
            timestamp: "2026-07-07T10:30:00Z",
            contextType: "review",
            attemptIndex: 2,
            hasConversation: false,
          })
        }
      >
        Select no transcript
      </button>
    </div>
  ),
}));

function task(overrides: Partial<Task> = {}): Task {
  return {
    id: "task-1",
    projectId: "project-1",
    category: "feature",
    title: "Historical chat task",
    description: "Verify stage transcript routing.",
    priority: 1,
    internalStatus: "merged",
    needsReviewPoint: false,
    createdAt: "2026-07-07T09:00:00Z",
    updatedAt: "2026-07-07T09:00:00Z",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
    ...overrides,
  };
}

function renderOverlay(
  props: Partial<ComponentProps<typeof AgentsTaskDetailOverlay>> = {},
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={0}>
        <AgentsTaskDetailOverlay
          projectId="project-1"
          selectedTaskIdOverride="task-1"
          {...props}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("AgentsTaskDetailOverlay historical runtime focus", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useTasksMock.mockReturnValue({ data: [task()] });
    useTaskMutationMock.mockReturnValue({
      updateMutation: { mutate: vi.fn(), isPending: false },
      moveMutation: { mutate: vi.fn(), isPending: false },
      archiveMutation: { mutate: vi.fn() },
      restoreMutation: { mutate: vi.fn() },
      isArchiving: false,
      isRestoring: false,
    });
    useUiStore.setState({
      taskHistoryState: null,
    });
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [
        { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      ],
      effectiveScopes: {},
      connectionPresentations: {},
    });
  });

  it("disables archive, shows the reason, and does not mutate while gated", async () => {
    const mutate = vi.fn();
    useTaskMutationMock.mockReturnValue({
      updateMutation: { mutate: vi.fn(), isPending: false },
      moveMutation: { mutate: vi.fn(), isPending: false },
      archiveMutation: { mutate },
      restoreMutation: { mutate: vi.fn() },
      isArchiving: false,
      isRestoring: false,
    });
    useEnvironmentStore.setState({
      activeEnvironmentId: "remote",
      environments: [{ id: "remote", name: "Remote", kind: "remote" }],
      effectiveScopes: { remote: ["ui:read", "ui:operate"] },
      connectionPresentations: {
        remote: {
          presentation: "connected",
          blockedFailure: null,
          blockedMessage: null,
        },
      },
    });
    renderOverlay();
    const button = screen.getByTestId("task-overlay-archive-button");
    // Soft-disabled so the reason stays reachable; a real `disabled` would hide it.
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).not.toBeDisabled();
    button.focus();
    expect(
      (await screen.findAllByText(/agent control/i)).length,
    ).toBeGreaterThan(0);
    fireEvent.click(button);
    expect(mutate).not.toHaveBeenCalled();
  });

  it("keeps archive live and mutates when the gate is granted", async () => {
    const mutate = vi.fn();
    useTaskMutationMock.mockReturnValue({
      updateMutation: { mutate: vi.fn(), isPending: false },
      moveMutation: { mutate: vi.fn(), isPending: false },
      archiveMutation: { mutate },
      restoreMutation: { mutate: vi.fn() },
      isArchiving: false,
      isRestoring: false,
    });
    useEnvironmentStore.setState({
      activeEnvironmentId: "remote",
      environments: [{ id: "remote", name: "Remote", kind: "remote" }],
      effectiveScopes: { remote: ["ui:read", "ui:operate", "ui:agent"] },
      connectionPresentations: {
        remote: {
          presentation: "connected",
          blockedFailure: null,
          blockedMessage: null,
        },
      },
    });
    renderOverlay();
    fireEvent.click(screen.getByTestId("task-overlay-archive-button"));
    fireEvent.click(await screen.findByRole("button", { name: "Archive" }));
    await waitFor(() =>
      expect(mutate).toHaveBeenCalledWith("task-1", expect.any(Object)),
    );
  });

  it.each([
    ["Select execution", "task_execution"],
    ["Select review", "review"],
    ["Select merge", "merge"],
  ] as const)(
    "focuses the main chat when a historical %s stage has a transcript",
    (buttonName, contextType) => {
      const onFocusTaskRuntime = vi.fn();

      renderOverlay({ onFocusTaskRuntime });

      fireEvent.click(screen.getByRole("button", { name: buttonName }));

      expect(onFocusTaskRuntime).toHaveBeenCalledWith("task-1", contextType);
      expect(screen.getByTestId("history-mode-banner")).toHaveTextContent(
        "Main chat is showing this runtime transcript",
      );
    },
  );

  it("keeps no-transcript history selection local to the detail panel", () => {
    const onFocusTaskRuntime = vi.fn();

    renderOverlay({ onFocusTaskRuntime });

    fireEvent.click(
      screen.getByRole("button", { name: "Select no transcript" }),
    );

    expect(onFocusTaskRuntime).not.toHaveBeenCalled();
    expect(screen.getByTestId("history-mode-banner")).toHaveTextContent(
      "No runtime transcript recorded for this stage",
    );
    expect(screen.getByTestId("mock-task-detail-panel")).toHaveAttribute(
      "data-view-status",
      "reviewing",
    );
  });

  it("keeps transcript-backed history local when the host does not provide a chat focus handler", () => {
    renderOverlay();

    fireEvent.click(screen.getByRole("button", { name: "Select execution" }));

    expect(screen.getByTestId("history-mode-banner")).toHaveTextContent(
      "Runtime transcript available",
    );
    expect(screen.getByTestId("mock-task-detail-panel")).toHaveAttribute(
      "data-view-status",
      "executing",
    );
    expect(screen.getByTestId("mock-task-detail-panel")).toHaveAttribute(
      "data-view-conversation-id",
      "exec-conversation",
    );
  });

  it("closes an open editor immediately when the host becomes read-only", () => {
    useTasksMock.mockReturnValue({
      data: [task({ internalStatus: "backlog" })],
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    function Harness() {
      const [readOnly, setReadOnly] = useState(false);
      return (
        <QueryClientProvider client={queryClient}>
          <TooltipProvider delayDuration={0}>
            <button type="button" onClick={() => setReadOnly(true)}>
              Make read-only
            </button>
            <AgentsTaskDetailOverlay
              projectId="project-1"
              selectedTaskIdOverride="task-1"
              readOnly={readOnly}
            />
          </TooltipProvider>
        </QueryClientProvider>
      );
    }
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Edit task" }));
    expect(screen.getByTestId("mock-task-edit-form")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Make read-only" }));

    expect(screen.queryByTestId("mock-task-edit-form")).not.toBeInTheDocument();
    expect(screen.getByTestId("mock-task-detail-panel")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Edit task" }),
    ).not.toBeInTheDocument();
  });
});
