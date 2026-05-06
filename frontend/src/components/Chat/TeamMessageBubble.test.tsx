/**
 * TeamMessageBubble tests
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { TeamMessageBubble } from "./TeamMessageBubble";

describe("TeamMessageBubble", () => {
  it("renders from, to, and content", () => {
    render(
      <TeamMessageBubble
        from="coder-1"
        to="coder-2"
        content="Session type is in auth.ts"
      />
    );
    expect(screen.getByText("coder-1")).toBeInTheDocument();
    expect(screen.getByText("coder-2")).toBeInTheDocument();
    expect(screen.getByText("Session type is in auth.ts")).toBeInTheDocument();
  });

  it("renders arrow separator", () => {
    render(
      <TeamMessageBubble from="a" to="b" content="test" />
    );
    expect(screen.getByText("→")).toBeInTheDocument();
  });

  it("renders color dot when fromColor is provided", () => {
    const { container } = render(
      <TeamMessageBubble from="coder-1" to="coder-2" content="test" fromColor="#3b82f6" />
    );
    const dot = container.querySelector("span.rounded-full");
    expect(dot).toBeInTheDocument();
    expect(dot).toHaveStyle({ backgroundColor: "#3b82f6" });
  });

  it("does not render color dot when fromColor is not provided", () => {
    const { container } = render(
      <TeamMessageBubble from="coder-1" to="coder-2" content="test" />
    );
    const dots = container.querySelectorAll("span.rounded-full");
    expect(dots.length).toBe(0);
  });
});

describe("TeamMessageBubble — branches & interactions", () => {
  let scrollHeightSpy: ReturnType<typeof vi.spyOn> | null = null;

  beforeEach(() => {
    scrollHeightSpy = null;
  });

  afterEach(() => {
    scrollHeightSpy?.mockRestore();
  });

  function mockScrollHeight(value: number) {
    scrollHeightSpy = vi
      .spyOn(HTMLDivElement.prototype, "scrollHeight", "get")
      .mockReturnValue(value);
  }

  it("renders 'all' label when to is wildcard", () => {
    mockScrollHeight(20);
    render(<TeamMessageBubble from="alpha" to="*" content="broadcast" />);
    expect(screen.getByText("all")).toBeInTheDocument();
  });

  it("renders timestamp formatted as time when provided", () => {
    mockScrollHeight(20);
    const ts = "2026-02-15T10:30:00Z";
    render(<TeamMessageBubble from="a" to="b" content="x" timestamp={ts} />);
    const expected = new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    expect(screen.getByText(expected)).toBeInTheDocument();
  });

  it("does not render timestamp when omitted", () => {
    mockScrollHeight(20);
    const { container } = render(<TeamMessageBubble from="a" to="b" content="x" />);
    // Header has 3 spans (from / arrow / to). With timestamp would have 4.
    const header = container.querySelector(".flex.items-center.gap-1\\.5");
    expect(header?.querySelectorAll("span").length).toBe(3);
  });

  it("does not render Show more when content fits within COLLAPSED_MAX_HEIGHT", () => {
    mockScrollHeight(50);
    render(<TeamMessageBubble from="a" to="b" content="short" />);
    expect(screen.queryByText("Show more")).not.toBeInTheDocument();
    expect(screen.queryByText("Show less")).not.toBeInTheDocument();
  });

  it("renders Show more when content exceeds COLLAPSED_MAX_HEIGHT", () => {
    mockScrollHeight(200);
    render(<TeamMessageBubble from="a" to="b" content="long content" />);
    expect(screen.getByText("Show more")).toBeInTheDocument();
  });

  it("toggles between Show more and Show less on click", () => {
    mockScrollHeight(200);
    render(<TeamMessageBubble from="a" to="b" content="long" />);
    fireEvent.click(screen.getByText("Show more"));
    expect(screen.getByText("Show less")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Show less"));
    expect(screen.getByText("Show more")).toBeInTheDocument();
  });

  it("hover handlers update toggle button color", () => {
    mockScrollHeight(200);
    render(<TeamMessageBubble from="a" to="b" content="long" />);
    const btn = screen.getByText("Show more") as HTMLButtonElement;
    fireEvent.mouseEnter(btn);
    expect(btn.style.color).toBe("var(--text-secondary)");
    fireEvent.mouseLeave(btn);
    expect(btn.style.color).toBe("var(--text-muted)");
  });

  it("renders gradient overlay when collapsed", () => {
    mockScrollHeight(200);
    const { container } = render(<TeamMessageBubble from="a" to="b" content="long" />);
    const grads = Array.from(container.querySelectorAll("div")).filter((d) =>
      (d.style.background || "").includes("linear-gradient"),
    );
    expect(grads.length).toBeGreaterThan(0);
  });

  it("hides gradient overlay when expanded", () => {
    mockScrollHeight(200);
    const { container } = render(<TeamMessageBubble from="a" to="b" content="long" />);
    fireEvent.click(screen.getByText("Show more"));
    const grads = Array.from(container.querySelectorAll("div")).filter((d) =>
      (d.style.background || "").includes("linear-gradient"),
    );
    expect(grads.length).toBe(0);
  });

  it("re-evaluates collapse threshold when content changes", () => {
    mockScrollHeight(20);
    const { rerender } = render(<TeamMessageBubble from="a" to="b" content="short" />);
    expect(screen.queryByText("Show more")).not.toBeInTheDocument();
    scrollHeightSpy?.mockRestore();
    mockScrollHeight(200);
    act(() => {
      rerender(<TeamMessageBubble from="a" to="b" content="now-long" />);
    });
    expect(screen.getByText("Show more")).toBeInTheDocument();
  });
});
