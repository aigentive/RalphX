import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ReviewArtifactTabs } from "./ReviewArtifactTabs";

describe("ReviewArtifactTabs", () => {
  it("always renders Overview then Requested Changes and changes documents", () => {
    const onValueChange = vi.fn();
    render(
      <ReviewArtifactTabs
        value="overview"
        onValueChange={onValueChange}
      />,
    );

    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual([
      "Overview",
      "Requested Changes",
    ]);

    fireEvent.mouseDown(
      screen.getByRole("tab", { name: "Requested Changes" }),
    );
    fireEvent.click(screen.getByRole("tab", { name: "Requested Changes" }));
    expect(onValueChange).toHaveBeenCalledWith("requested_changes");
  });
});
