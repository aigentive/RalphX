import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PlanBundleTabs } from "./PlanBundleTabs";

describe("PlanBundleTabs", () => {
  it("always renders overview and blueprint and hides empty proposals", () => {
    render(
      <PlanBundleTabs
        idPrefix="test-plan"
        value="overview"
        onValueChange={() => undefined}
        linkedProposalsCount={0}
      />,
    );

    expect(screen.getByRole("tab", { name: "Overview" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "Blueprint" })).toBeVisible();
    expect(screen.queryByRole("tab", { name: /Proposals/ })).toBeNull();
  });

  it("preserves the proposal visibility condition and changes modes", () => {
    const onValueChange = vi.fn();
    render(
      <PlanBundleTabs
        idPrefix="test-plan"
        value="overview"
        onValueChange={onValueChange}
        linkedProposalsCount={2}
      />,
    );

    fireEvent.mouseDown(screen.getByRole("tab", { name: "Blueprint" }));
    fireEvent.click(screen.getByRole("tab", { name: "Blueprint" }));
    expect(onValueChange).toHaveBeenCalledWith("blueprint");
    expect(screen.getByRole("tab", { name: "Proposals (2)" })).toBeVisible();
  });
});
