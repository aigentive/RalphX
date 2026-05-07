import React from "react";
import { ReactFlowProvider } from "@xyflow/react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TierGroup, type TierGroupData } from "./TierGroup";

function renderTierGroup(data: Partial<TierGroupData> = {}, selected = false) {
  const merged: TierGroupData = {
    tierGroupId: "tier-1",
    planArtifactId: "plan-1",
    tier: 1,
    taskIds: ["task-1"],
    isCollapsed: false,
    width: 600,
    height: 240,
    onToggleCollapse: vi.fn(),
    ...data,
  };

  const props = {
    id: merged.tierGroupId,
    type: "tierGroup",
    selected,
    data: merged,
    isConnectable: false,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
    zIndex: 0,
  } as unknown as React.ComponentProps<typeof TierGroup>;

  return render(
    <ReactFlowProvider>
      <TierGroup {...props} />
    </ReactFlowProvider>,
  );
}

describe("TierGroup wrapper", () => {
  it("uses an opaque elevated surface instead of a transparent mix", () => {
    renderTierGroup();

    const wrapper = screen.getByTestId("tier-group-tier-1");
    expect(wrapper.className).toContain("bg-[var(--bg-elevated)]");
    expect(wrapper.className).not.toContain("transparent");
  });
});
