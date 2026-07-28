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

import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  AGENT_CONTROL_DISABLED_HINT,
  resolveAffordanceGate,
} from "@/lib/remote/agent-gate";

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
// Wiring guard for surfaces whose hosts are too heavy to mount
// ---------------------------------------------------------------------------

/**
 * Every A3 surface must consult the ONE gate hook. Mounting each host means standing
 * up the agents shell, the automation panel's query tree, or a DndContext, so the
 * guard is structural: it proves the gate is still wired, and the behavioural
 * columns above prove the wiring produces the right outcome.
 */
describe("gated surfaces stay wired to the gate hook", () => {
  const files = [
    "src/components/agents/AgentComposerSurface.tsx",
    "src/components/Chat/ChatInput.tsx",
    "src/components/PermissionDialog.tsx",
    "src/hooks/useQuestionInput.ts",
    "src/components/tasks/TaskBoard/TaskBoard.tsx",
    "src/components/tasks/TaskBoard/Column.tsx",
    "src/components/tasks/TaskContextMenuItems.tsx",
    "src/components/tasks/GroupContextMenuItems.tsx",
    "src/components/tasks/detail-views/HumanReviewTaskDetail.tsx",
    "src/components/tasks/detail-views/EscalatedTaskDetail.tsx",
    "src/components/tasks/detail-views/BasicTaskDetail.tsx",
    "src/components/tasks/TaskEditForm.tsx",
    "src/components/agents/task-details/TaskEditForm.tsx",
    "src/components/agents/task-details/StepList.tsx",
    "src/components/Ideation/PlanEditor.tsx",
    "src/components/Ideation/ProposalCard.tsx",
    "src/components/Ideation/ProposalDetailSheet.tsx",
    "src/components/agents/AgentsAutomationPanel.tsx",
    "src/hooks/useIdeation.ts",
  ];

  it.each(files)("%s consults useAgentGate", async (file) => {
    const { readFileSync } = await import("node:fs");
    const { resolve } = await import("node:path");
    const source = readFileSync(resolve(__dirname, "../../..", file), "utf8");
    expect(source).toContain('from "@/hooks/useAgentGate"');
    // Named affordance or the scope-only fallback — both are the one hook.
    expect(source).toMatch(/use(?:Agent|Field)Gate\(/);
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
