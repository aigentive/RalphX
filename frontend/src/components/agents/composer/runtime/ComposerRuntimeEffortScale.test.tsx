import { useState } from "react";

import { fireEvent, render, screen } from "@testing-library/react";
import { TooltipProvider } from "@radix-ui/react-tooltip";
import { describe, expect, it, vi } from "vitest";

import { ComposerRuntimeEffortScale } from "./ComposerRuntimeEffortScale";

const options = [
  { id: "quick", label: "Quick", description: "Prioritize speed." },
  { id: "balanced", label: "Balanced", description: "Balance depth." },
  { id: "deep", label: "Deep", description: "Prioritize reasoning." },
];

function MirroredScales({ onCommit }: { onCommit: (value: string) => void }) {
  const [previewIndex, setPreviewIndex] = useState<number | null>(null);
  return (
    <TooltipProvider>
      <ComposerRuntimeEffortScale
        value="quick"
        options={options}
        previewIndex={previewIndex}
        onPreviewChange={setPreviewIndex}
        onCommit={onCommit}
      />
      <ComposerRuntimeEffortScale
        value="quick"
        options={options}
        previewIndex={previewIndex}
        onPreviewChange={setPreviewIndex}
        onCommit={onCommit}
      />
    </TooltipProvider>
  );
}

describe("ComposerRuntimeEffortScale", () => {
  it("mirrors pointer preview between scales and commits one option on release", () => {
    const onCommit = vi.fn();
    render(<MirroredScales onCommit={onCommit} />);
    const sliders = screen.getAllByRole("slider", { name: "Effort" });
    const first = sliders[0]!;
    vi.spyOn(first, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 0,
      width: 100,
      height: 28,
      top: 0,
      right: 100,
      bottom: 28,
      left: 0,
      toJSON: () => ({}),
    });

    fireEvent.pointerDown(first, { clientX: 100, pointerId: 1 });
    expect(sliders[0]).toHaveAttribute("aria-valuetext", "Deep");
    expect(sliders[1]).toHaveAttribute("aria-valuetext", "Deep");
    expect(onCommit).not.toHaveBeenCalled();

    fireEvent.pointerUp(first, { clientX: 100, pointerId: 1 });
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith("deep");
  });

  it("renders a single option as a centered noninteractive scale", () => {
    const onCommit = vi.fn();
    render(
      <TooltipProvider>
        <ComposerRuntimeEffortScale
          value="only"
          options={[{ id: "only", label: "Only" }]}
          previewIndex={null}
          onPreviewChange={vi.fn()}
          onCommit={onCommit}
        />
      </TooltipProvider>,
    );

    const slider = screen.getByRole("slider", { name: "Effort" });
    expect(slider).toHaveAttribute("aria-disabled", "true");
    expect(slider).toHaveAttribute("aria-valuetext", "Only");
    fireEvent.keyDown(slider, { key: "ArrowRight" });
    expect(onCommit).not.toHaveBeenCalled();
  });
});
