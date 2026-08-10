/**
 * The 2.6-b surface matrix: local × remote-granted × remote-absent.
 *
 * The remote-absent column's assertions are the ones that matter, and they are
 * ABSENCE assertions on the mutation layer — a test that only checked `disabled` on a
 * button would pass against a component that still dispatched on Enter, on a
 * keyboard activation, or through a stale callback reference. Where a surface is too
 * heavy to mount, the gate is asserted at the seam it actually shares with the
 * component (the same hook, the same reason string) plus a wiring guard.
 */

import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  AGENT_CONTROL_DISABLED_HINT,
  resolveAffordanceGate,
} from "@/lib/remote/agent-gate";
import {
  GATE_WIRED_FILES,
  quarantinedIds,
} from "./agent-gate-guard-manifest";

const REMOTE_ID = "env-remote";

type Column = "local" | "granted" | "absent";

function setColumn(column: Column): void {
  if (column === "local") {
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
      effectiveScopes: {},
      connectionPresentations: {},
    });
    return;
  }
  useEnvironmentStore.setState({
    activeEnvironmentId: REMOTE_ID,
    // Healthy connection: 2.7-a's read-only mode outranks the scope answer, so these
    // scope-dimension cases hold the connection dimension fixed.
    connectionPresentations: {
      [REMOTE_ID]: {
        presentation: "connected",
        blockedFailure: null,
        blockedMessage: null,
      },
    },
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      { id: REMOTE_ID, name: "Studio Mac", kind: "remote" },
    ],
    effectiveScopes: {
      [REMOTE_ID]:
        column === "granted"
          ? ["ui:read", "ui:operate", "ui:agent"]
          : ["ui:read", "ui:operate"],
    },
  });
}

function withTooltips(node: React.ReactNode) {
  return <TooltipProvider>{node}</TooltipProvider>;
}

beforeEach(() => {
  vi.clearAllMocks();
  setColumn("local");
});

// ---------------------------------------------------------------------------
// Chat send — scope-gated remotely since send_remote_chat_message was registered
// ---------------------------------------------------------------------------

describe("chat send", () => {
  async function renderInput(onSend: () => void) {
    const { ChatInput } = await import("@/components/Chat/ChatInput");
    render(
      withTooltips(<ChatInput value="hello" onChange={vi.fn()} onSend={onSend} />)
    );
  }

  it("local: send is enabled and dispatches", async () => {
    const onSend = vi.fn();
    await renderInput(onSend);

    const button = screen.getByTestId("chat-input-send");
    expect(button).toBeEnabled();
    button.click();
    expect(onSend).toHaveBeenCalled();
  });

  // Chat send used to be `unavailable` in BOTH remote columns, because the affordance
  // pointed at `send_agent_message` and no spawn-free send existed. Registering
  // `send_remote_chat_message` makes the two columns differ, which is the whole point:
  // the device is now told the truth about WHY it cannot send, and granting ui:agent
  // now actually changes the answer.

  it("remote/granted: send is enabled and dispatches", async () => {
    setColumn("granted");
    const onSend = vi.fn();
    await renderInput(onSend);

    const button = screen.getByTestId("chat-input-send");
    expect(button).toBeEnabled();
    button.click();
    expect(onSend).toHaveBeenCalled();
  });

  it("remote/absent: send is SCOPE-gated, not unavailable, and never dispatches", async () => {
    setColumn("absent");
    const onSend = vi.fn();
    await renderInput(onSend);

    const button = screen.getByTestId("chat-input-send");
    expect(button).toBeDisabled();
    button.click();
    expect(onSend).not.toHaveBeenCalled();

    const tooltip = screen.getByTestId("agent-gate-tooltip");
    expect(tooltip).toHaveAttribute("data-agent-gated", "true");

    // Which of the two disabled reasons this is, asserted at the seam that decides it —
    // the hint itself lives in Radix tooltip content that only mounts on hover. The
    // distinction is the point: "enable it on the host" is now TRUE advice, where
    // before the op was absent and it would have been a lie.
    expect(resolveAffordanceGate("chatSend", true, ["ui:read", "ui:operate"])).toEqual({
      status: "gated",
      gated: true,
      reason: AGENT_CONTROL_DISABLED_HINT,
    });
    expect(
      resolveAffordanceGate("chatSend", true, ["ui:read", "ui:operate", "ui:agent"])
        .status
    ).toBe("enabled");
  });
});

// ---------------------------------------------------------------------------
// Task edit form — the ARGUMENT-level gate (title/description vs category/priority)
// ---------------------------------------------------------------------------

describe("task edit form field-level gate", () => {
  const task = {
    id: "task-1",
    title: "Original title",
    description: "Original description",
    category: "feature",
    priority: 3,
    internalStatus: "backlog",
    projectId: "project-1",
    archivedAt: null,
  };

  async function renderForm(onSave: (data: unknown) => void) {
    const { QueryClient, QueryClientProvider } = await import(
      "@tanstack/react-query"
    );
    const { TaskEditForm } = await import("@/components/tasks/TaskEditForm");
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        {withTooltips(
          <TaskEditForm task={task as never} onSave={onSave} onCancel={vi.fn()} />
        )}
      </QueryClientProvider>
    );
  }

  it("local: title and description are editable", async () => {
    await renderForm(vi.fn());
    expect(screen.getByLabelText("Title")).toBeEnabled();
    expect(screen.getByLabelText("Description")).toBeEnabled();
  });

  it("absent: agent-consumed fields lock, category and priority stay editable", async () => {
    setColumn("absent");
    await renderForm(vi.fn());

    // The brakes half of the boundary must survive the gate.
    expect(screen.getByLabelText("Title")).toBeDisabled();
    expect(screen.getByLabelText("Description")).toBeDisabled();
    expect(screen.getByLabelText("Category")).toBeEnabled();
    expect(screen.getByLabelText("Priority")).toBeEnabled();
  });

  it("granted: everything is editable again", async () => {
    setColumn("granted");
    await renderForm(vi.fn());
    expect(screen.getByLabelText("Title")).toBeEnabled();
    expect(screen.getByLabelText("Description")).toBeEnabled();
  });
});

// ---------------------------------------------------------------------------
// Context menus — the gated item must EXPLAIN itself
//
// A Radix `disabled` menu item gets `pointer-events: none` from the shared item
// styles and leaves the roving-focus order, so the `title` these items used to carry
// could never be shown by either mouse or keyboard. The items are now soft-disabled
// (`aria-disabled` + intercepted activation) and wrapped in the app tooltip.
// ---------------------------------------------------------------------------

describe("task context menu gated action", () => {
  async function renderMenu(onStartExecution: () => void) {
    const {
      ContextMenu,
      ContextMenuContent,
      ContextMenuTrigger,
    } = await import("@/components/ui/context-menu");
    const {
      TaskContextMenuItems,
      TaskContextMenuDialogs,
      TaskContextMenuProvider,
      useTaskContextMenu,
    } = await import("@/components/tasks/TaskContextMenuItems");

    const task = {
      id: "task-1",
      projectId: "project-1",
      title: "Test Task",
      description: "",
      category: "feature",
      priority: 3,
      internalStatus: "ready",
      archivedAt: null,
    };
    const handlers = { onViewDetails: vi.fn(), onStartExecution };

    function Harness() {
      const state = useTaskContextMenu();
      return (
        <TaskContextMenuProvider state={state}>
          <ContextMenu>
            <ContextMenuTrigger asChild>
              <div data-testid="trigger">Trigger</div>
            </ContextMenuTrigger>
            <ContextMenuContent>
              <TaskContextMenuItems
                task={task as never}
                handlers={handlers as never}
                context="kanban"
              />
            </ContextMenuContent>
            <TaskContextMenuDialogs task={task as never} handlers={handlers as never} />
          </ContextMenu>
        </TaskContextMenuProvider>
      );
    }

    render(
      <TooltipProvider delayDuration={0}>
        <Harness />
      </TooltipProvider>
    );
    const { fireEvent } = await import("@testing-library/react");
    fireEvent.contextMenu(screen.getByTestId("trigger"));
  }

  it("local: Start Execution is a normal, activatable item", async () => {
    await renderMenu(vi.fn());
    const item = screen.getByTestId("start-action");
    expect(item).not.toHaveAttribute("aria-disabled");
    expect(item).not.toHaveAttribute("data-agent-gated");
  });

  it("remote/absent: the item is inert but its reason is reachable", async () => {
    setColumn("absent");
    const onStartExecution = vi.fn();
    await renderMenu(onStartExecution);

    const item = screen.getByTestId("start-action");
    expect(item).toHaveAttribute("data-agent-gated", "true");
    expect(item).toHaveAttribute("aria-disabled", "true");
    // Absence assertion on the mechanism, not just on the outcome: `data-disabled` is
    // what kills pointer events and keyboard focus.
    expect(item).not.toHaveAttribute("data-disabled");

    // Still refuses to steer, on the mouse path and the keyboard path.
    const { fireEvent } = await import("@testing-library/react");
    fireEvent.click(item);
    item.focus();
    fireEvent.keyDown(item, { key: "Enter" });
    expect(onStartExecution).not.toHaveBeenCalled();

    // ...and the explanation is now actually reachable — by focus, which is the
    // keyboard path a `title` never had.
    await waitFor(() => {
      expect(screen.getByTestId("start-gate-explanation")).toHaveTextContent(
        AGENT_CONTROL_DISABLED_HINT
      );
    });
  });
});

describe("group context menu gated action", () => {
  async function renderMenu(onResumeAll: () => void) {
    const {
      ContextMenu,
      ContextMenuContent,
      ContextMenuTrigger,
    } = await import("@/components/ui/context-menu");
    const { GroupContextMenuItems } = await import(
      "@/components/tasks/GroupContextMenuItems"
    );

    render(
      <TooltipProvider delayDuration={0}>
        <ContextMenu>
          <ContextMenuTrigger asChild>
            <div data-testid="trigger">Trigger</div>
          </ContextMenuTrigger>
          <ContextMenuContent>
            <GroupContextMenuItems
              groupLabel="Paused"
              groupKind="column"
              taskCount={2}
              projectId="project-1"
              groupId="paused"
              onResumeAll={onResumeAll}
              confirm={vi.fn().mockResolvedValue(true)}
            />
          </ContextMenuContent>
        </ContextMenu>
      </TooltipProvider>
    );
    const { fireEvent } = await import("@testing-library/react");
    fireEvent.contextMenu(screen.getByTestId("trigger"));
  }

  it("local: Resume All stays a normal item", async () => {
    await renderMenu(vi.fn());
    const item = screen.getByTestId("resume-all-action");
    expect(item).not.toHaveAttribute("aria-disabled");
  });

  it("remote/absent: Resume All is inert with a reachable reason", async () => {
    setColumn("absent");
    const onResumeAll = vi.fn();
    await renderMenu(onResumeAll);

    const item = screen.getByTestId("resume-all-action");
    expect(item).toHaveAttribute("aria-disabled", "true");
    expect(item).not.toHaveAttribute("data-disabled");

    const { fireEvent } = await import("@testing-library/react");
    fireEvent.click(item);
    item.focus();
    fireEvent.keyDown(item, { key: "Enter" });
    expect(onResumeAll).not.toHaveBeenCalled();

    await waitFor(() => {
      expect(
        screen.getByTestId("resume-all-gate-explanation")
      ).toBeInTheDocument();
    });
  });
});

// ---------------------------------------------------------------------------
// Wiring guard for surfaces whose hosts are too heavy to mount
// ---------------------------------------------------------------------------

/**
 * Every A3 surface must consult the ONE gate hook. Mounting each host means standing
 * up the agents shell, the automation panel's query tree, or a DndContext, so the
 * guard is structural: it proves the gate is still wired, and the behavioural
 * columns above prove the wiring produces the right outcome.
 */
describe("gated surfaces stay wired to the gate hook", () => {
  async function readSource(file: string): Promise<string> {
    const { readFileSync } = await import("node:fs");
    const { resolve } = await import("node:path");
    return readFileSync(resolve(__dirname, "../../..", file), "utf8");
  }

  function isWired(source: string): boolean {
    return (
      source.includes('from "@/hooks/useAgentGate"') &&
      /use(?:Agent|Field)Gate\(/.test(source)
    );
  }

  const quarantined = quarantinedIds("wiring");
  const enforced = GATE_WIRED_FILES.filter((file) => !quarantined.has(`wiring:${file}`));
  const expectedUngated = GATE_WIRED_FILES.filter((file) =>
    quarantined.has(`wiring:${file}`)
  );

  it.each(enforced)("%s consults useAgentGate", async (file) => {
    const source = await readSource(file);
    expect(source).toContain('from "@/hooks/useAgentGate"');
    // Named affordance or the scope-only fallback — both are the one hook.
    expect(source).toMatch(/use(?:Agent|Field)Gate\(/);
  });

  // The ratchet half. These are the Agents-pane detail-view fork — the surface the
  // remote-primary Agents pane actually renders, which shipped with no gate at all. They
  // are listed above so the file list can never silently omit the fork again, and pinned
  // as failing here so Phase 4 cannot fix one without deleting its KNOWN_GATE_GAPS row.
  it.each(expectedUngated)("%s is still ungated (ratchet)", async (file) => {
    const source = await readSource(file);
    expect(
      isWired(source),
      `${file} is now gated — delete wiring:${file} from KNOWN_GATE_GAPS`
    ).toBe(false);
  });

  it("keeps the inert brake surfaces free of the gate", async () => {
    const { readFileSync } = await import("node:fs");
    const { resolve } = await import("node:path");
    // Backlog quick-add and the execution control bar's stop/pause are A6 inert:
    // wiring the gate into them would be a boundary regression.
    for (const file of [
      "src/components/tasks/InlineTaskAdd.tsx",
      "src/components/tasks/TaskBoard/CollapsedQuickAdd.tsx",
    ]) {
      const source = readFileSync(resolve(__dirname, "../../..", file), "utf8");
      expect(source, file).not.toContain("useAgentGate");
    }
  });
});

// ---------------------------------------------------------------------------
// Copy is fixed by contract
// ---------------------------------------------------------------------------

describe("gate copy", () => {
  it("uses the exact contract wording", () => {
    expect(AGENT_CONTROL_DISABLED_HINT).toBe(
      "Agent control is off for this device — enable it on the host."
    );
  });
});
