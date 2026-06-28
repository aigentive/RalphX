import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PublishWorkspaceDialog } from "./AgentsPublishWorkspaceDialog";

describe("PublishWorkspaceDialog", () => {
  it("describes a new pull request target when no PR is linked", () => {
    render(
      <PublishWorkspaceDialog
        base="main"
        branch="ralphx/demo/agent-123"
        confirmDisabled={false}
        isPublishing={false}
        onConfirm={vi.fn()}
        onOpenChange={vi.fn()}
        open
        phase="confirm"
        status={null}
      />,
    );

    expect(
      screen.getByText(
        "This will commit workspace changes on ralphx/demo/agent-123 and push them to a pull request against main.",
      ),
    ).toBeInTheDocument();
  });

  it("describes updating a linked pull request", () => {
    render(
      <PublishWorkspaceDialog
        base="main"
        branch="feature/linked-pr"
        confirmDisabled={false}
        isPublishing={false}
        onConfirm={vi.fn()}
        onOpenChange={vi.fn()}
        open
        phase="confirm"
        status={null}
        targetPullRequestLabel="PR #42"
      />,
    );

    expect(
      screen.getByText(
        "This will commit workspace changes on feature/linked-pr and push updates to PR #42.",
      ),
    ).toBeInTheDocument();
  });
});
