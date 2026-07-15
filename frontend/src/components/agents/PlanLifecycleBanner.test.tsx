import { render, screen, within } from "@testing-library/react";
import { Pause, Play, Square } from "lucide-react";
import { describe, expect, it, vi } from "vitest";

import { PlanLifecycleBanner } from "./PlanLifecycleBanner";

describe("PlanLifecycleBanner", () => {
  it.each([
    ["needs_approval", "var(--status-warning)", "Plan needs approval"],
    ["approved", "var(--status-info)", "Plan approved"],
    ["accepted", "var(--status-success)", "Plan accepted"],
  ] as const)(
    "maps %s to the expected lifecycle accent token",
    (state, accentToken, title) => {
      render(
        <PlanLifecycleBanner
          state={state}
          title={title}
          description="Lifecycle state"
          actions={[]}
          counts={
            state === "accepted"
              ? {
                  total: 2,
                  idle: 0,
                  active: 1,
                  done: 1,
                }
              : undefined
          }
          onViewWork={state === "accepted" ? vi.fn() : undefined}
        />,
      );

      const banner = screen.getByTestId("plan-lifecycle-banner");

      expect(banner).toHaveAttribute("data-lifecycle-state", state);
      expect(
        banner.style.getPropertyValue("--plan-lifecycle-accent"),
      ).toBe(accentToken);
      expect(within(banner).getByText(title)).toBeInTheDocument();
    },
  );

  it("renders accepted work progress and View Work only for accepted state", () => {
    const onViewWork = vi.fn();

    render(
      <PlanLifecycleBanner
        state="accepted"
        title="Plan accepted"
        description="Implementation work is ready."
        actions={[]}
        counts={{
          total: 3,
          idle: 1,
          active: 1,
          done: 1,
        }}
        onViewWork={onViewWork}
      />,
    );

    const banner = screen.getByTestId("plan-lifecycle-banner");

    expect(within(banner).getByText("3 tasks")).toBeInTheDocument();
    expect(within(banner).getByText("1 in progress")).toBeInTheDocument();
    expect(within(banner).getByText("1 completed")).toBeInTheDocument();
    within(banner).getByRole("button", { name: /View Work/i }).click();

    expect(onViewWork).toHaveBeenCalledTimes(1);
  });

  it("renders accepted footer controls and dispatches callbacks", () => {
    const onPause = vi.fn();
    const onResume = vi.fn();
    const onStop = vi.fn();

    render(
      <PlanLifecycleBanner
        state="accepted"
        title="Plan accepted"
        description="Implementation work is ready."
        actions={[]}
        counts={{
          total: 3,
          idle: 0,
          active: 2,
          done: 1,
        }}
        acceptedFooterActions={[
          {
            key: "pause",
            label: "Pause",
            icon: Pause,
            onClick: onPause,
            testId: "plan-pause-button",
          },
          {
            key: "resume",
            label: "Resume",
            icon: Play,
            onClick: onResume,
            testId: "plan-resume-button",
          },
          {
            key: "stop",
            label: "Stop",
            icon: Square,
            onClick: onStop,
            testId: "plan-stop-button",
          },
        ]}
      />,
    );

    const banner = screen.getByTestId("plan-lifecycle-banner");
    within(banner).getByRole("button", { name: /Pause/i }).click();
    within(banner).getByRole("button", { name: /Resume/i }).click();
    within(banner).getByRole("button", { name: /Stop/i }).click();

    expect(onPause).toHaveBeenCalledTimes(1);
    expect(onResume).toHaveBeenCalledTimes(1);
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it("uses runtime counts so paused tasks are not shown as in progress", () => {
    render(
      <PlanLifecycleBanner
        state="accepted"
        title="Plan accepted"
        description="Implementation work is ready."
        actions={[]}
        counts={{
          total: 3,
          idle: 2,
          active: 1,
          done: 0,
        }}
        acceptedRuntimeCounts={{
          running: 0,
          paused: 1,
        }}
      />,
    );

    const banner = screen.getByTestId("plan-lifecycle-banner");

    expect(within(banner).queryByText("1 in progress")).not.toBeInTheDocument();
    expect(within(banner).getByText("1 paused")).toBeInTheDocument();
    expect(within(banner).getByText("2 queued")).toBeInTheDocument();
  });
});
