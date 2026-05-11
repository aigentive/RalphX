import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { markPostUpdatePreparing } from "@/lib/postUpdatePreparing";
import { usePostUpdatePreparing } from "@/hooks/usePostUpdatePreparing";
import { PostUpdatePreparingScreen } from "./PostUpdatePreparingScreen";

function Harness({
  ready = true,
  minDurationMs = 20,
  maxDurationMs = 1_000,
}: {
  ready?: boolean;
  minDurationMs?: number;
  maxDurationMs?: number;
}) {
  const isPreparing = usePostUpdatePreparing(ready, {
    minDurationMs,
    maxDurationMs,
  });

  return isPreparing ? (
    <PostUpdatePreparingScreen />
  ) : (
    <div data-testid="ready-ui">Ready UI</div>
  );
}

describe("PostUpdatePreparingScreen", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000);
    localStorage.clear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("does not replace the app shell without a post-update marker", () => {
    render(<Harness />);

    expect(screen.queryByTestId("post-update-preparing")).not.toBeInTheDocument();
    expect(screen.getByTestId("ready-ui")).toBeInTheDocument();
  });

  it("shows the preparing screen for a fresh post-update marker", () => {
    markPostUpdatePreparing("0.12.3", Date.now());

    render(<Harness />);

    expect(screen.getByTestId("post-update-preparing")).toHaveTextContent(
      "Preparing RalphX",
    );
    expect(screen.queryByTestId("ready-ui")).not.toBeInTheDocument();
  });

  it("dismisses after the app is ready and the minimum display time has elapsed", async () => {
    markPostUpdatePreparing("0.12.3", Date.now());

    render(<Harness minDurationMs={20} />);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(25);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(50);
    });

    expect(screen.getByTestId("ready-ui")).toBeInTheDocument();
  });

  it("uses the maximum duration as an escape hatch if readiness stalls", async () => {
    markPostUpdatePreparing("0.12.3", Date.now());

    render(<Harness ready={false} maxDurationMs={50} />);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(60);
    });

    expect(screen.getByTestId("ready-ui")).toBeInTheDocument();
  });
});
