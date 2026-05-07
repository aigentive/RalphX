/**
 * Tests for PlanGroup wrapper styling and selection outline.
 */

import React from "react";
import { ReactFlowProvider } from "@xyflow/react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PlanGroup, type PlanGroupData } from "./PlanGroup";

vi.mock("./PlanGroupHeader", () => ({
  PlanGroupHeader: ({ planArtifactId }: { planArtifactId: string }) => (
    <div data-testid={`plan-group-header-${planArtifactId}`}>header</div>
  ),
}));

vi.mock("@/components/tasks/GroupContextMenuItems", () => ({
  GroupContextMenuItems: () => null,
}));

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: () => ({
    confirm: vi.fn(),
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  }),
}));

function renderPlanGroup(
  data: Partial<PlanGroupData> = {},
  selected = false,
) {
  const merged: PlanGroupData = {
    planArtifactId: "plan-1",
    sessionId: "sess-1",
    sessionTitle: "Plan Title",
    taskIds: ["t1"],
    statusSummary: { ready: 1, executing: 0, in_review: 0, done: 0, blocked: 0 } as PlanGroupData["statusSummary"],
    isCollapsed: false,
    width: 600,
    height: 200,
    ...data,
  };
  const props = {
    id: merged.planArtifactId,
    type: "planGroup",
    selected,
    data: merged,
    isConnectable: false,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
    zIndex: 0,
  } as unknown as React.ComponentProps<typeof PlanGroup>;
  return render(
    <ReactFlowProvider>
      <PlanGroup {...props} />
    </ReactFlowProvider>,
  );
}

describe("PlanGroup wrapper", () => {
  it("paints an opaque surface so the graph grid does not show through", () => {
    renderPlanGroup();
    const wrapper = screen.getByTestId("plan-group-plan-1");
    expect(wrapper.style.backgroundColor).toBe("var(--bg-surface)");
  });

  it("applies a 2px accent outline when the group is selected", () => {
    renderPlanGroup({}, true);
    const wrapper = screen.getByTestId("plan-group-plan-1");
    expect(wrapper.style.outline).toContain("var(--accent-primary)");
    expect(wrapper.style.outlineOffset).toBe("-2px");
  });

  it("collapses to header height when isCollapsed is true", () => {
    renderPlanGroup({ isCollapsed: true, height: 800 });
    const wrapper = screen.getByTestId("plan-group-plan-1");
    // displayHeight = HEADER_HEIGHT + 8; ensure the collapsed height is used (not 800).
    expect(wrapper.style.height).not.toBe("800px");
  });
});
