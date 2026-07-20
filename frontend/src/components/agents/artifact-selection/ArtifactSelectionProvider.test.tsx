import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { toast } from "sonner";

import { ArtifactSelectableRegion } from "./ArtifactSelectableRegion";
import { ArtifactSelectionProvider } from "./ArtifactSelectionProvider";

vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));

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

  it("rejects unregistered lifecycle text and dismisses a previously open action", () => {
    render(
      <ArtifactSelectionProvider enabled onAddExcerpt={vi.fn()}>
        <ArtifactSelectableRegion
          source={{
            sourceKind: "plan",
            sourceId: "artifact-1",
            sourceLabel: "Plan",
            version: 4,
          }}
        >
          <p>Ship the native selection flow</p>
        </ArtifactSelectableRegion>
        <p>Plan needs approval</p>
      </ArtifactSelectionProvider>,
    );

    const planText = screen.getByText("Ship the native selection flow");
    mockSelection(planText.firstChild!, "Ship the native selection flow");
    fireEvent.pointerUp(planText);
    expect(
      screen.getByRole("button", { name: "Add selection to conversation" }),
    ).toBeInTheDocument();

    const lifecycleText = screen.getByText("Plan needs approval");
    mockSelection(lifecycleText.firstChild!, "Plan needs approval");
    fireEvent.pointerUp(lifecycleText);
    expect(
      screen.queryByRole("button", { name: "Add selection to conversation" }),
    ).not.toBeInTheDocument();
  });

  it("dismisses a pending action when its registered region unmounts", () => {
    const { rerender } = render(
      <ArtifactSelectionProvider enabled onAddExcerpt={vi.fn()}>
        <ArtifactSelectableRegion
          source={{ sourceKind: "plan", sourceId: "artifact-1", sourceLabel: "Plan" }}
        >
          <p>Ship the native selection flow</p>
        </ArtifactSelectableRegion>
      </ArtifactSelectionProvider>,
    );

    const planText = screen.getByText("Ship the native selection flow");
    mockSelection(planText.firstChild!, "Ship the native selection flow");
    fireEvent.pointerUp(planText);
    expect(
      screen.getByRole("button", { name: "Add selection to conversation" }),
    ).toBeInTheDocument();

    rerender(
      <ArtifactSelectionProvider enabled onAddExcerpt={vi.fn()}>
        <p>Plan needs approval</p>
      </ArtifactSelectionProvider>,
    );

    expect(
      screen.queryByRole("button", { name: "Add selection to conversation" }),
    ).not.toBeInTheDocument();
  });

  it("does not offer an action when selection crosses from a region into chrome", () => {
    render(
      <ArtifactSelectionProvider enabled onAddExcerpt={vi.fn()}>
        <ArtifactSelectableRegion
          source={{ sourceKind: "plan", sourceId: "artifact-1", sourceLabel: "Plan" }}
        >
          <p>Ship the native selection flow</p>
        </ArtifactSelectableRegion>
        <p>Plan needs approval</p>
      </ArtifactSelectionProvider>,
    );

    const planText = screen.getByText("Ship the native selection flow").firstChild!;
    const lifecycleText = screen.getByText("Plan needs approval").firstChild!;
    const range = {
      cloneContents: () => document.createDocumentFragment(),
      getBoundingClientRect: () => ({ bottom: 80, left: 40, width: 140 }),
    } as unknown as Range;
    vi.spyOn(window, "getSelection").mockReturnValue({
      anchorNode: planText,
      focusNode: lifecycleText,
      isCollapsed: false,
      rangeCount: 1,
      getRangeAt: () => range,
      toString: () => "Ship the native selection flow Plan needs approval",
    } as unknown as Selection);

    fireEvent.pointerUp(screen.getByText("Plan needs approval"));
    expect(
      screen.queryByRole("button", { name: "Add selection to conversation" }),
    ).not.toBeInTheDocument();
  });

  function renderRegion(onAdd = vi.fn()) {
    render(
      <ArtifactSelectionProvider enabled onAddExcerpt={onAdd}>
        <ArtifactSelectableRegion
          source={{ sourceKind: "plan", sourceId: "artifact-1", sourceLabel: "Plan" }}
        >
          <p>Ship the native selection flow</p>
        </ArtifactSelectableRegion>
      </ArtifactSelectionProvider>,
    );
    return screen.getByText("Ship the native selection flow");
  }

  it("does not offer an action when the provider is disabled", () => {
    render(
      <ArtifactSelectionProvider enabled={false} onAddExcerpt={vi.fn()}>
        <ArtifactSelectableRegion
          source={{ sourceKind: "plan", sourceId: "artifact-1", sourceLabel: "Plan" }}
        >
          <p>Ship the native selection flow</p>
        </ArtifactSelectableRegion>
      </ArtifactSelectionProvider>,
    );

    const text = screen.getByText("Ship the native selection flow");
    mockSelection(text.firstChild!, "Ship the native selection flow");
    fireEvent.pointerUp(text);

    expect(
      screen.queryByRole("button", { name: "Add selection to conversation" }),
    ).not.toBeInTheDocument();
  });

  it("does not offer an action for a collapsed selection", () => {
    const text = renderRegion();
    vi.spyOn(window, "getSelection").mockReturnValue({
      anchorNode: text.firstChild,
      focusNode: text.firstChild,
      isCollapsed: true,
      rangeCount: 1,
    } as unknown as Selection);

    fireEvent.pointerUp(text);
    expect(
      screen.queryByRole("button", { name: "Add selection to conversation" }),
    ).not.toBeInTheDocument();
  });

  it("does not offer an action when the selection contains interactive content", () => {
    const text = renderRegion();
    const fragment = document.createDocumentFragment();
    fragment.appendChild(document.createElement("button"));
    const range = {
      cloneContents: () => fragment,
      getBoundingClientRect: () => ({ bottom: 80, height: 20, left: 40, width: 140 }),
    } as unknown as Range;
    vi.spyOn(window, "getSelection").mockReturnValue({
      anchorNode: text.firstChild,
      focusNode: text.firstChild,
      isCollapsed: false,
      rangeCount: 1,
      getRangeAt: () => range,
      toString: () => "Ship the native selection flow",
    } as unknown as Selection);

    fireEvent.pointerUp(text);
    expect(
      screen.queryByRole("button", { name: "Add selection to conversation" }),
    ).not.toBeInTheDocument();
  });

  it("does not offer an action when selection endpoints are inside a control", () => {
    render(
      <ArtifactSelectionProvider enabled onAddExcerpt={vi.fn()}>
        <ArtifactSelectableRegion
          source={{ sourceKind: "plan", sourceId: "artifact-1", sourceLabel: "Plan" }}
        >
          <button type="button">Approve Plan</button>
        </ArtifactSelectableRegion>
      </ArtifactSelectionProvider>,
    );

    const button = screen.getByRole("button", { name: "Approve Plan" });
    mockSelection(button.firstChild!, "Approve Plan");
    fireEvent.pointerUp(button);

    expect(
      screen.queryByRole("button", { name: "Add selection to conversation" }),
    ).not.toBeInTheDocument();
  });

  it("rejects and warns when the selection exceeds the byte limit", () => {
    const text = renderRegion();
    mockSelection(text.firstChild!, "x".repeat(16 * 1024 + 1));
    fireEvent.pointerUp(text);

    expect(
      screen.queryByRole("button", { name: "Add selection to conversation" }),
    ).not.toBeInTheDocument();
    expect(toast.error).toHaveBeenCalledWith(
      "Selection is too large to add as conversation context",
    );
  });

  it("dismisses the action on Escape and on viewport scroll", () => {
    const text = renderRegion();
    mockSelection(text.firstChild!, "Ship the native selection flow");
    fireEvent.pointerUp(text);
    expect(
      screen.getByRole("button", { name: "Add selection to conversation" }),
    ).toBeInTheDocument();

    fireEvent.keyDown(document, { key: "Escape" });
    expect(
      screen.queryByRole("button", { name: "Add selection to conversation" }),
    ).not.toBeInTheDocument();

    mockSelection(text.firstChild!, "Ship the native selection flow");
    fireEvent.pointerUp(text);
    expect(
      screen.getByRole("button", { name: "Add selection to conversation" }),
    ).toBeInTheDocument();

    fireEvent.scroll(window);
    expect(
      screen.queryByRole("button", { name: "Add selection to conversation" }),
    ).not.toBeInTheDocument();
  });

  it("dismisses the action on an outside pointer press", () => {
    const text = renderRegion();
    mockSelection(text.firstChild!, "Ship the native selection flow");
    fireEvent.pointerUp(text);
    expect(
      screen.getByRole("button", { name: "Add selection to conversation" }),
    ).toBeInTheDocument();

    fireEvent.pointerDown(document.body);
    expect(
      screen.queryByRole("button", { name: "Add selection to conversation" }),
    ).not.toBeInTheDocument();
  });
});
