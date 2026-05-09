import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ReviewTimeline } from "./ReviewTimeline";
import type { ReviewNoteResponse } from "@/lib/tauri";
import type { StateTransition } from "@/api/tasks";

describe("ReviewTimeline", () => {
  it("renders optional entry context labels", () => {
    const history: ReviewNoteResponse[] = [
      {
        id: "note-task",
        task_id: "task-123",
        reviewer: "ai",
        outcome: "approved",
        summary: "Looks good.",
        notes: null,
        created_at: new Date().toISOString(),
      },
    ];

    render(
      <ReviewTimeline
        history={history}
        stateTransitions={[] satisfies StateTransition[]}
        getEntryContext={() => "Fix graph crash"}
      />
    );

    expect(screen.getByText("Fix graph crash")).toBeInTheDocument();
  });

  it("shows summary preview and full dialog for large system feedback", async () => {
    const user = userEvent.setup();
    const history: ReviewNoteResponse[] = [
      {
        id: "note-hook",
        task_id: "task-123",
        reviewer: "system",
        outcome: "changes_requested",
        summary: "Repository commit hooks rejected the merge commit.",
        notes: [
          "Repository commit hooks rejected the merge commit.",
          "",
          "Full hook output:",
          "```text",
          "\u001b[31m[pre-commit]\u001b[0m design-token guards failed",
          ...Array.from({ length: 240 }, (_, index) => `TS2307 Cannot find module 'zod' — extended diagnostic ${index}`),
          "```",
        ].join("\n"),
        created_at: new Date().toISOString(),
      },
    ];

    render(
      <ReviewTimeline history={history} stateTransitions={[] satisfies StateTransition[]} />
    );

    expect(
      screen.getByText("System requested changes")
    ).toBeInTheDocument();
    expect(
      screen.getByText("Repository commit hooks rejected the merge commit.")
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "View full feedback" }));

    expect(screen.getByText("Full review feedback")).toBeInTheDocument();
    expect(screen.getByText(/design-token guards failed/)).toBeInTheDocument();
    expect(
      screen.queryByText((content) => content.includes("\u001b[31m"))
    ).not.toBeInTheDocument();
  });
});
