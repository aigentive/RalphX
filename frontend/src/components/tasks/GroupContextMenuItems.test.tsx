import { resetTransportEnvironmentId } from "@/lib/remote/active-environment";
import { resetQueryClient } from "@/lib/queryClient";
import { createElement, type ReactElement } from "react";
/**
 * GroupContextMenuItems.test.tsx - Tests for group-level bulk action context menu items
 */

import { afterEach, describe, it, expect, vi, beforeEach } from "vitest";
import {
  render as rtlRender,
  screen,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { GroupContextMenuItems } from "./GroupContextMenuItems";
import { useConfirmation } from "@/hooks/useConfirmation";

// Gate tests park the store on a remote environment; without this the next file in
// the same worker inherits it and resolves a different keyed QueryClient. That is
// what broke EnvironmentScopedProviders under CI sharding.
afterEach(() => {
  resetQueryClient();
  resetTransportEnvironmentId();
  useEnvironmentStore.setState({ activeEnvironmentId: LOCAL_ENVIRONMENT_ID });
});

import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";

// Gated icon-only controls carry the app tooltip; `Tooltip` throws without a
// provider, which production gets from App.tsx.
function render(ui: ReactElement): ReturnType<typeof rtlRender> {
  return rtlRender(createElement(TooltipProvider, null, ui));
}

// ============================================================================
// Helpers
// ============================================================================

function TestWrapper({
  groupLabel,
  groupKind,
  taskCount,
  projectId = "project-1",
  groupId = "ready",
  onArchiveAll,
  onCancelAll,
  onPauseAll,
}: {
  groupLabel: string;
  groupKind: "column" | "plan" | "uncategorized";
  taskCount: number;
  projectId?: string;
  groupId?: string;
  onArchiveAll?: () => void;
  onCancelAll?: () => void;
  onPauseAll?: () => void;
}) {
  const { confirm, confirmationDialogProps, ConfirmationDialog } =
    useConfirmation();
  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <div data-testid="trigger">Trigger</div>
        </ContextMenuTrigger>
        <ContextMenuContent>
          <GroupContextMenuItems
            groupLabel={groupLabel}
            groupKind={groupKind}
            taskCount={taskCount}
            projectId={projectId}
            groupId={groupId}
            onArchiveAll={onArchiveAll}
            onCancelAll={onCancelAll}
            onPauseAll={onPauseAll}

            confirm={confirm}
          />
        </ContextMenuContent>
      </ContextMenu>
      <ConfirmationDialog {...confirmationDialogProps} />
    </>
  );
}

function openContextMenu() {
  fireEvent.contextMenu(screen.getByTestId("trigger"));
}

// ============================================================================
// Tests
// ============================================================================

describe("GroupContextMenuItems", () => {
  let onArchiveAll: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    onArchiveAll = vi.fn();
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [
        { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      ],
      effectiveScopes: {},
      connectionPresentations: {},
    });
  });

  it.each([
    ["pause-all-action", "pause-all-gate-explanation"],
    ["cancel-all-action", "cancel-all-gate-explanation"],
  ])(
    "gates %s, shows its reason, and does not dispatch",
    async (actionId, explanationId) => {
      const onPauseAll = vi.fn();
      const onCancelAll = vi.fn();
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
      render(
        <TestWrapper
          groupLabel="Ready"
          groupKind="column"
          taskCount={2}
          onPauseAll={onPauseAll}
          onCancelAll={onCancelAll}
        />,
      );
      openContextMenu();
      const action = screen.getByTestId(actionId);
      expect(action).toHaveAttribute("data-agent-gated", "true");
      // The reason lives in a Radix TooltipContent, which only mounts once the
      // trigger is focused — the same pattern agent-gate-surfaces.test.tsx uses.
      action.focus();
      await waitFor(() => {
        expect(screen.getByTestId(explanationId)).toHaveTextContent(
          /agent control/i,
        );
      });
      fireEvent.click(action);
      expect(onPauseAll).not.toHaveBeenCalled();
      expect(onCancelAll).not.toHaveBeenCalled();
    },
  );

  it("keeps pause and cancel live and dispatches after confirmation when granted", async () => {
    const onPauseAll = vi.fn();
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
    render(
      <TestWrapper
        groupLabel="Ready"
        groupKind="column"
        taskCount={2}
        onPauseAll={onPauseAll}
      />,
    );
    openContextMenu();
    fireEvent.click(screen.getByTestId("pause-all-action"));
    fireEvent.click(await screen.findByRole("button", { name: "Pause" }));
    await waitFor(() => expect(onPauseAll).toHaveBeenCalledTimes(1));
  });

  describe("rendering", () => {
    it("renders 'Archive all Ready' for column kind", () => {
      render(
        <TestWrapper
          groupLabel="Ready"
          groupKind="column"
          taskCount={3}
          onArchiveAll={onArchiveAll}
        />,
      );
      openContextMenu();
      expect(screen.getByText("Archive all Ready")).toBeInTheDocument();
    });

    it("renders 'Archive all in [Plan]' for plan kind", () => {
      render(
        <TestWrapper
          groupLabel="Auth Feature"
          groupKind="plan"
          taskCount={5}
          groupId="session-abc"
          onArchiveAll={onArchiveAll}
        />,
      );
      openContextMenu();
      expect(
        screen.getByText("Archive all in Auth Feature"),
      ).toBeInTheDocument();
    });

    it("renders 'Archive all Uncategorized' for uncategorized kind", () => {
      render(
        <TestWrapper
          groupLabel=""
          groupKind="uncategorized"
          taskCount={2}
          onArchiveAll={onArchiveAll}
        />,
      );
      openContextMenu();
      expect(screen.getByText("Archive all Uncategorized")).toBeInTheDocument();
    });

    it("renders nothing when taskCount is 0", () => {
      render(
        <TestWrapper
          groupLabel="Ready"
          groupKind="column"
          taskCount={0}
          onArchiveAll={onArchiveAll}
        />,
      );
      openContextMenu();
      expect(screen.queryByText(/Archive all/)).not.toBeInTheDocument();
    });

    it("has data-testid for archive-all action", () => {
      render(
        <TestWrapper
          groupLabel="Ready"
          groupKind="column"
          taskCount={3}
          onArchiveAll={onArchiveAll}
        />,
      );
      openContextMenu();
      expect(screen.getByTestId("archive-all-action")).toBeInTheDocument();
    });

    it("renders nothing when no handlers provided", () => {
      render(
        <TestWrapper groupLabel="Ready" groupKind="column" taskCount={3} />,
      );
      openContextMenu();
      expect(screen.queryByText(/Archive all/)).not.toBeInTheDocument();
    });
  });

  describe("confirmation flow", () => {
    it("shows confirmation dialog when archive-all clicked", async () => {
      render(
        <TestWrapper
          groupLabel="Ready"
          groupKind="column"
          taskCount={3}
          onArchiveAll={onArchiveAll}
        />,
      );
      openContextMenu();
      fireEvent.click(screen.getByText("Archive all Ready"));

      await waitFor(() => {
        expect(screen.getByText("Archive all Ready?")).toBeInTheDocument();
      });
      expect(screen.getByText(/3 tasks/)).toBeInTheDocument();
    });

    it("calls onArchiveAll when confirmed", async () => {
      render(
        <TestWrapper
          groupLabel="Blocked"
          groupKind="column"
          taskCount={2}
          groupId="blocked"
          onArchiveAll={onArchiveAll}
        />,
      );
      openContextMenu();
      fireEvent.click(screen.getByText("Archive all Blocked"));

      await waitFor(() => {
        expect(screen.getByText("Archive all Blocked?")).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText("Archive"));
      await waitFor(() => {
        expect(onArchiveAll).toHaveBeenCalledTimes(1);
      });
    });

    it("does not call onArchiveAll when cancelled", async () => {
      render(
        <TestWrapper
          groupLabel="Ready"
          groupKind="column"
          taskCount={3}
          onArchiveAll={onArchiveAll}
        />,
      );
      openContextMenu();
      fireEvent.click(screen.getByText("Archive all Ready"));

      await waitFor(() => {
        expect(screen.getByText("Archive all Ready?")).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText("Cancel"));
      await waitFor(() => {
        expect(
          screen.queryByText("Archive all Ready?"),
        ).not.toBeInTheDocument();
      });
      expect(onArchiveAll).not.toHaveBeenCalled();
    });
  });
});
