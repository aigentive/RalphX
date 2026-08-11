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
    fireEvent.keyDown(slider, { key: "ArrowDown" });
    fireEvent.keyDown(slider, { key: "End" });
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("clears keyboard previews with Escape without committing", () => {
    const onPreviewChange = vi.fn();
    const onCommit = vi.fn();
    render(
      <TooltipProvider>
        <ComposerRuntimeEffortScale
          value="balanced"
          options={options}
          previewIndex={2}
          onPreviewChange={onPreviewChange}
          onCommit={onCommit}
        />
      </TooltipProvider>,
    );

    const slider = screen.getByRole("slider", { name: "Effort" });
    expect(slider).toHaveAttribute("aria-valuetext", "Deep");

    fireEvent.keyDown(slider, { key: "Escape" });

    expect(onPreviewChange).toHaveBeenCalledWith(null);
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("ignores disabled pointer and keyboard interactions", () => {
    const onPreviewChange = vi.fn();
    const onCommit = vi.fn();
    render(
      <TooltipProvider>
        <ComposerRuntimeEffortScale
          value="balanced"
          options={options}
          previewIndex={null}
          onPreviewChange={onPreviewChange}
          onCommit={onCommit}
          disabled
        />
      </TooltipProvider>,
    );

    const slider = screen.getByRole("slider", { name: "Effort" });
    fireEvent.pointerDown(slider, { clientX: 100, pointerId: 1 });
    fireEvent.pointerUp(slider, { clientX: 100, pointerId: 1 });
    fireEvent.keyDown(slider, { key: "End" });

    expect(slider).toHaveAttribute("aria-disabled", "true");
    expect(onPreviewChange).not.toHaveBeenCalled();
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("commits pointer release position when no preview was captured", () => {
    const onCommit = vi.fn();
    render(
      <TooltipProvider>
        <ComposerRuntimeEffortScale
          value="quick"
          options={options}
          previewIndex={null}
          onPreviewChange={vi.fn()}
          onCommit={onCommit}
        />
      </TooltipProvider>,
    );

    const slider = screen.getByRole("slider", { name: "Effort" });
    vi.spyOn(slider, "getBoundingClientRect").mockReturnValue({
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

    fireEvent.pointerUp(slider, { clientX: 100, pointerId: 1 });

    expect(onCommit).toHaveBeenCalledWith("deep");
  });

  it("updates pointer previews while captured and clears them on cancel", () => {
    const onPreviewChange = vi.fn();
    render(
      <TooltipProvider>
        <ComposerRuntimeEffortScale
          value="quick"
          options={options}
          previewIndex={null}
          onPreviewChange={onPreviewChange}
          onCommit={vi.fn()}
        />
      </TooltipProvider>,
    );

    const slider = screen.getByRole("slider", { name: "Effort" });
    vi.spyOn(slider, "getBoundingClientRect").mockReturnValue({
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
    slider.hasPointerCapture = vi.fn(() => true);

    fireEvent.pointerMove(slider, { clientX: 50, pointerId: 1 });
    fireEvent.pointerCancel(slider, { pointerId: 1 });

    expect(onPreviewChange.mock.calls).toEqual([[1], [null]]);
  });

  it("returns null when effort has no options", () => {
    const { container } = render(
      <TooltipProvider>
        <ComposerRuntimeEffortScale
          value=""
          options={[]}
          previewIndex={null}
          onPreviewChange={vi.fn()}
          onCommit={vi.fn()}
        />
      </TooltipProvider>,
    );

    expect(container).toBeEmptyDOMElement();
  });
});
