/**
 * TimeoutWarning component tests
 * Tests: renders at 70% threshold, no render below, dismiss, configurable thresholds
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, act, fireEvent } from "@testing-library/react";
import { TimeoutWarning } from "./TimeoutWarning";

describe("TimeoutWarning", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("does not render when elapsed time is below 70% threshold", () => {
    const effectiveTimeoutMs = 600_000; // 600s
    // Start time 60% through (360s ago) — below 70% threshold
    const toolCallStartTime = Date.now() - effectiveTimeoutMs * 0.6;

    render(
      <TimeoutWarning
        toolCallStartTime={toolCallStartTime}
        effectiveTimeoutMs={effectiveTimeoutMs}
        onDismiss={vi.fn()}
      />
    );

    expect(screen.queryByTestId("timeout-warning-banner")).toBeNull();
  });

  it("renders warning when elapsed time reaches 70% threshold", () => {
    const effectiveTimeoutMs = 600_000; // 600s
    // Start time exactly at 70% (420s ago)
    const toolCallStartTime = Date.now() - effectiveTimeoutMs * 0.7;

    render(
      <TimeoutWarning
        toolCallStartTime={toolCallStartTime}
        effectiveTimeoutMs={effectiveTimeoutMs}
        onDismiss={vi.fn()}
      />
    );

    expect(screen.getByTestId("timeout-warning-banner")).toBeInTheDocument();
  });

  it("renders warning when elapsed time exceeds 70% threshold", () => {
    const effectiveTimeoutMs = 600_000; // 600s
    // Start time at 85% (510s ago) — well above threshold
    const toolCallStartTime = Date.now() - effectiveTimeoutMs * 0.85;

    render(
      <TimeoutWarning
        toolCallStartTime={toolCallStartTime}
        effectiveTimeoutMs={effectiveTimeoutMs}
        onDismiss={vi.fn()}
      />
    );

    expect(screen.getByTestId("timeout-warning-banner")).toBeInTheDocument();
  });

  it("reveals warning after timer fires and threshold is crossed", () => {
    const effectiveTimeoutMs = 600_000; // 600s
    // Start time at 65% (390s ago) — just below threshold
    const toolCallStartTime = Date.now() - effectiveTimeoutMs * 0.65;

    render(
      <TimeoutWarning
        toolCallStartTime={toolCallStartTime}
        effectiveTimeoutMs={effectiveTimeoutMs}
        onDismiss={vi.fn()}
      />
    );

    // Not shown yet
    expect(screen.queryByTestId("timeout-warning-banner")).toBeNull();

    // Advance time by 5s (CHECK_INTERVAL_MS) — now at ~66% still below threshold
    act(() => {
      vi.advanceTimersByTime(5_000);
    });

    // Still not shown (65% + ~0.83% = ~65.8%)
    expect(screen.queryByTestId("timeout-warning-banner")).toBeNull();

    // Advance another 35s to push past 70%
    act(() => {
      vi.advanceTimersByTime(35_000);
    });

    expect(screen.getByTestId("timeout-warning-banner")).toBeInTheDocument();
  });

  it("calls onDismiss when dismiss button is clicked", () => {
    const onDismiss = vi.fn();
    const effectiveTimeoutMs = 600_000;
    // Already at 80% — will render immediately
    const toolCallStartTime = Date.now() - effectiveTimeoutMs * 0.8;

    render(
      <TimeoutWarning
        toolCallStartTime={toolCallStartTime}
        effectiveTimeoutMs={effectiveTimeoutMs}
        onDismiss={onDismiss}
      />
    );

    const dismissBtn = screen.getByLabelText("Dismiss timeout warning");
    fireEvent.click(dismissBtn);

    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("uses a configured 600s threshold", () => {
    const effectiveTimeoutMs = 600_000;
    // At exactly 70% = 420s
    const toolCallStartTime = Date.now() - 420_000;

    render(
      <TimeoutWarning
        toolCallStartTime={toolCallStartTime}
        effectiveTimeoutMs={effectiveTimeoutMs}
        onDismiss={vi.fn()}
      />
    );

    expect(screen.getByTestId("timeout-warning-banner")).toBeInTheDocument();
    // Banner should mention 600s as timeout
    expect(screen.getByTestId("timeout-warning-banner")).toHaveTextContent("600s");
  });

  it("uses a configured 3600s threshold", () => {
    const effectiveTimeoutMs = 3_600_000;
    // At exactly 70% = 2520s
    const toolCallStartTime = Date.now() - 2_520_000;

    render(
      <TimeoutWarning
        toolCallStartTime={toolCallStartTime}
        effectiveTimeoutMs={effectiveTimeoutMs}
        onDismiss={vi.fn()}
      />
    );

    expect(screen.getByTestId("timeout-warning-banner")).toBeInTheDocument();
    // Banner should mention 3600s as timeout
    expect(screen.getByTestId("timeout-warning-banner")).toHaveTextContent("3600s");
  });

  it("does not render below 70% of a configured 3600s threshold", () => {
    const effectiveTimeoutMs = 3_600_000;
    // At 60% = 2160s — below threshold
    const toolCallStartTime = Date.now() - 2_160_000;

    render(
      <TimeoutWarning
        toolCallStartTime={toolCallStartTime}
        effectiveTimeoutMs={effectiveTimeoutMs}
        onDismiss={vi.fn()}
      />
    );

    expect(screen.queryByTestId("timeout-warning-banner")).toBeNull();
  });
});
