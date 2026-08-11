import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { AgentsPublishActionBar } from "./AgentsPublishActionBar";

describe("AgentsPublishActionBar", () => {
  it.each([
    ["neutral", "Checking workspace changes"],
    ["warning", "Ready to publish"],
    ["success", "Published to GitHub"],
    ["error", "Publishing failed"],
  ] as const)("renders the %s presentation with semantic status", (tone, title) => {
    render(
      <AgentsPublishActionBar
        presentation={{
          title,
          summary: "Presentation detail",
          tone,
        }}
        primaryAction={<button type="button">Primary action</button>}
      />,
    );

    const actionBar = screen.getByTestId("agents-publish-actionbar");
    expect(actionBar).toHaveAttribute("data-tone", tone);
    expect(actionBar.style.backgroundColor).not.toBe("");
    expect(actionBar.style.borderColor).not.toBe("");
    expect(actionBar.style.borderStyle).toBe("solid");
    expect(actionBar.style.borderWidth).toBe("0px 0px 1px");
    expect(screen.getByRole("heading", { name: title })).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-status-icon")).toBeInTheDocument();
  });

  it("uses a busy status icon without replacing the accessible title", () => {
    render(
      <AgentsPublishActionBar
        presentation={{
          title: "Publishing workspace",
          summary: "Publishing is in progress.",
          tone: "neutral",
          busy: true,
        }}
        primaryAction={<button type="button">Publishing</button>}
      />,
    );

    expect(
      screen.getByRole("heading", { name: "Publishing workspace" }),
    ).toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-status-icon")).toHaveClass(
      "animate-spin",
    );
  });

  it("keeps maintenance announcements stable until the operation identity changes", async () => {
    const { rerender } = render(
      <AgentsPublishActionBar
        presentation={{
          title: "Repairing workspace",
          summary: "Resolving the base conflict.",
          tone: "warning",
          busy: true,
        }}
        liveAnnouncement={{
          operationKey: "maintenance-1:1:repairing",
          title: "Repairing workspace",
          summary: "Resolving the base conflict.",
        }}
        primaryAction={<button type="button">Repairing</button>}
      />,
    );

    const announcement = await screen.findByRole("status");
    expect(announcement).toHaveTextContent("Repairing workspace");
    expect(announcement).toHaveTextContent("Resolving the base conflict.");

    rerender(
      <AgentsPublishActionBar
        presentation={{
          title: "Repairing workspace",
          summary: "Still resolving the base conflict.",
          tone: "warning",
          busy: true,
        }}
        liveAnnouncement={{
          operationKey: "maintenance-1:1:repairing",
          title: "Repairing workspace",
          summary: "Still resolving the base conflict.",
        }}
        primaryAction={<button type="button">Repairing</button>}
      />,
    );

    expect(
      screen.getByText("Still resolving the base conflict.", { selector: "p" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("status")).toBe(announcement);
    expect(announcement).toHaveTextContent("Resolving the base conflict.");
    expect(announcement).not.toHaveTextContent(
      "Still resolving the base conflict.",
    );

    for (const nextAnnouncement of [
      {
        operationKey: "maintenance-1:1:validating",
        title: "Validating repair",
        summary: "Checking the repaired workspace.",
      },
      {
        operationKey: "maintenance-1:2:validating",
        title: "Validating repair",
        summary: "Checking the next repair generation.",
      },
      {
        operationKey: "maintenance-2:1:validating",
        title: "Validating repair",
        summary: "Checking the replacement repair operation.",
      },
    ]) {
      rerender(
        <AgentsPublishActionBar
          presentation={{
            title: nextAnnouncement.title,
            summary: nextAnnouncement.summary,
            tone: "neutral",
            busy: true,
          }}
          liveAnnouncement={nextAnnouncement}
          primaryAction={<button type="button">Validating</button>}
        />,
      );

      await waitFor(() =>
        expect(announcement).toHaveTextContent(nextAnnouncement.summary),
      );
      expect(announcement).toHaveTextContent(nextAnnouncement.title);
    }
  });

  it("renders known change facts and omits unknown facts", () => {
    const { rerender } = render(
      <AgentsPublishActionBar
        presentation={{
          title: "Ready to publish",
          summary: "Changes are ready.",
          tone: "warning",
        }}
        changeFacts={{ fileCount: 3, additions: 8, deletions: 2 }}
        primaryAction={<button type="button">Commit &amp; Publish</button>}
      />,
    );

    expect(screen.getByTestId("agents-publish-change-facts")).toHaveTextContent(
      "3 files",
    );
    expect(screen.getByTestId("agents-publish-additions")).toHaveTextContent("+8");
    expect(screen.getByTestId("agents-publish-deletions")).toHaveTextContent("−2");

    rerender(
      <AgentsPublishActionBar
        presentation={{
          title: "Checking workspace changes",
          summary: "Loading changed files...",
          tone: "neutral",
        }}
        primaryAction={<button type="button">Commit &amp; Publish</button>}
      />,
    );

    expect(
      screen.queryByTestId("agents-publish-change-facts"),
    ).not.toBeInTheDocument();
  });

  it("composes the shared automation status pill and action slots", () => {
    render(
      <AgentsPublishActionBar
        presentation={{
          title: "Automatic publishing paused",
          summary: "Manual publishing remains available.",
          tone: "warning",
        }}
        automationStatus={{
          label: "Auto Publish paused",
          tone: "warning",
        }}
        primaryAction={<button type="button">Commit &amp; Publish</button>}
        overflowAction={
          <button type="button" aria-label="Publish actions">
            More
          </button>
        }
      />,
    );

    expect(screen.getByTestId("agents-pr-supervision-status")).toHaveAttribute(
      "data-tone",
      "warning",
    );
    expect(screen.getByRole("button", { name: "Commit & Publish" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Publish actions" })).toBeEnabled();
  });
});
