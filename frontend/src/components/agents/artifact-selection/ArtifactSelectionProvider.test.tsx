import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ArtifactSelectableRegion } from "./ArtifactSelectableRegion";
import { ArtifactSelectionProvider } from "./ArtifactSelectionProvider";

function mockSelection(node: Node, text: string) {
  const range = {
    commonAncestorContainer: node,
    cloneContents: () => document.createDocumentFragment(),
    getBoundingClientRect: () => ({
      bottom: 80,
      height: 20,
      left: 40,
      right: 180,
      top: 60,
      width: 140,
      x: 40,
      y: 60,
      toJSON: () => ({}),
    }),
  } as unknown as Range;
  vi.spyOn(window, "getSelection").mockReturnValue({
    anchorNode: node,
    focusNode: node,
    isCollapsed: false,
    rangeCount: 1,
    getRangeAt: () => range,
    removeAllRanges: vi.fn(),
    toString: () => text,
  } as unknown as Selection);
}

describe("ArtifactSelectionProvider", () => {
  it("offers selected rendered text and stages a plain-text snapshot", () => {
    const onAdd = vi.fn();
    render(
      <ArtifactSelectionProvider enabled onAddExcerpt={onAdd}>
        <ArtifactSelectableRegion
          source={{
            sourceKind: "plan",
            sourceId: "artifact-1",
            sourceLabel: "Plan",
            title: "Release plan",
            version: 4,
          }}
        >
          <p>Ship the native selection flow</p>
        </ArtifactSelectableRegion>
      </ArtifactSelectionProvider>,
    );

    const text = screen.getByText("Ship the native selection flow");
    mockSelection(text.firstChild!, "Ship the native selection flow");
    fireEvent.pointerUp(text);

    fireEvent.click(
      screen.getByRole("button", { name: "Add selection to conversation" }),
    );
    expect(onAdd).toHaveBeenCalledWith({
      sourceKind: "plan",
      sourceId: "artifact-1",
      sourceLabel: "Plan",
      title: "Release plan",
      excerpt: "Ship the native selection flow",
      version: 4,
    });
  });

  it("does not offer an action when selection endpoints cross logical sources", () => {
    render(
      <ArtifactSelectionProvider enabled onAddExcerpt={vi.fn()}>
        <ArtifactSelectableRegion
          source={{ sourceKind: "issue", sourceId: "one", sourceLabel: "Issue" }}
        >
          <p>First issue</p>
        </ArtifactSelectableRegion>
        <ArtifactSelectableRegion
          source={{ sourceKind: "issue", sourceId: "two", sourceLabel: "Issue" }}
        >
          <p>Second issue</p>
        </ArtifactSelectableRegion>
      </ArtifactSelectionProvider>,
    );

    const first = screen.getByText("First issue").firstChild!;
    const second = screen.getByText("Second issue").firstChild!;
    const range = {
      cloneContents: () => document.createDocumentFragment(),
      getBoundingClientRect: () => ({ bottom: 80, left: 40, width: 140 }),
    } as unknown as Range;
    vi.spyOn(window, "getSelection").mockReturnValue({
      anchorNode: first,
      focusNode: second,
      isCollapsed: false,
      rangeCount: 1,
      getRangeAt: () => range,
      toString: () => "First issue Second issue",
    } as unknown as Selection);

    fireEvent.pointerUp(screen.getByText("Second issue"));
    expect(
      screen.queryByRole("button", { name: "Add selection to conversation" }),
    ).not.toBeInTheDocument();
  });
});
