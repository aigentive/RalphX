import { fireEvent, render, screen } from "@testing-library/react";
import { CheckCheck, Clock } from "lucide-react";
import { describe, expect, it, vi } from "vitest";

import {
  AgentsInboxGroupEmptyStrip,
  AgentsInboxZeroCard,
} from "./AgentsInboxEmptyState";

describe("AgentsInboxZeroCard", () => {
  it("renders the caller identity, headline, and supporting copy", () => {
    render(
      <AgentsInboxZeroCard
        testId="agents-inbox-lane-empty-recent"
        tone="win"
        icon={CheckCheck}
        headline="Inbox zero"
        subline="Nothing needs you, nothing is running."
      />
    );

    expect(screen.getByTestId("agents-inbox-lane-empty-recent")).toHaveTextContent(
      "Inbox zero"
    );
    expect(screen.getByText("Nothing needs you, nothing is running.")).toBeInTheDocument();
  });

  it("keeps calm marks visually distinct from the win treatment", () => {
    const view = render(
      <AgentsInboxZeroCard
        testId="empty-tone"
        tone="win"
        icon={CheckCheck}
        headline="Inbox zero"
        subline="All clear."
      />
    );
    const winMark = screen.getByTestId("empty-tone").querySelector("svg")?.parentElement;
    expect(winMark).toHaveStyle({ backgroundColor: "var(--accent-muted)" });

    view.rerender(
      <AgentsInboxZeroCard
        testId="empty-tone"
        tone="calm"
        icon={Clock}
        headline="Nothing has gone stale"
        subline="Nothing is drifting."
      />
    );
    const calmMark = screen.getByTestId("empty-tone").querySelector("svg")?.parentElement;
    expect(calmMark).toHaveStyle({ backgroundColor: "var(--overlay-faint)" });
    expect(calmMark).not.toHaveStyle({ backgroundColor: "var(--accent-muted)" });
  });

  it("omits unavailable actions and invokes the actions it receives", () => {
    const onPrimary = vi.fn();
    const onSecondary = vi.fn();
    const view = render(
      <AgentsInboxZeroCard
        testId="empty-actions"
        tone="win"
        icon={CheckCheck}
        headline="Inbox zero"
        subline="All clear."
      />
    );

    expect(screen.queryByRole("button")).not.toBeInTheDocument();

    view.rerender(
      <AgentsInboxZeroCard
        testId="empty-actions"
        tone="win"
        icon={CheckCheck}
        headline="Inbox zero"
        subline="All clear."
        primaryAction={{ label: "New agent", onClick: onPrimary }}
        secondaryAction={{ label: "Review 7 done", onClick: onSecondary }}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "New agent" }));
    fireEvent.click(screen.getByRole("button", { name: "Review 7 done" }));

    expect(onPrimary).toHaveBeenCalledOnce();
    expect(onSecondary).toHaveBeenCalledOnce();
  });
});

describe("AgentsInboxGroupEmptyStrip", () => {
  it("renders its label under the caller's stable test id", () => {
    render(
      <AgentsInboxGroupEmptyStrip
        testId="agents-inbox-lane-empty-needs"
        label="Nothing needs you"
      />
    );

    expect(screen.getByTestId("agents-inbox-lane-empty-needs")).toHaveTextContent(
      "Nothing needs you"
    );
  });
});
