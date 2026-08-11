import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { PublishPipelineSteps } from "./AgentsPublishPipelineSteps";

describe("PublishPipelineSteps", () => {
  it("shows auto-merge spinner when supervision status is null (pending)", () => {
    const { container } = render(
      <PublishPipelineSteps
        status="pushed"
        isPublishing={false}
        autoMergeDesired
        autoMergeCurrent={false}
        prSupervisionStatus={null}
      />,
    );

    const step = screen.getByTestId("agents-publish-step-auto_merge");
    expect(step).toHaveTextContent("Request auto-merge");
    expect(container.querySelector(".animate-spin")).toBeInTheDocument();
  });

  it("shows deferred warning when supervision status is waiting", () => {
    const { container } = render(
      <PublishPipelineSteps
        status="pushed"
        isPublishing={false}
        autoMergeDesired
        autoMergeCurrent={false}
        prSupervisionStatus="waiting"
      />,
    );

    const step = screen.getByTestId("agents-publish-step-auto_merge");
    expect(step).toHaveTextContent("Auto-merge deferred");
    expect(container.querySelector(".animate-spin")).not.toBeInTheDocument();
  });

  it("shows auto-merge as done when autoMergeCurrent is true", () => {
    render(
      <PublishPipelineSteps
        status="pushed"
        isPublishing={false}
        autoMergeDesired
        autoMergeCurrent={true}
        prSupervisionStatus="monitoring"
      />,
    );

    const step = screen.getByTestId("agents-publish-step-auto_merge");
    expect(step).toHaveTextContent("Request auto-merge");
  });

  it("omits the auto-merge step when auto-merge was not requested", () => {
    render(<PublishPipelineSteps status="pushed" isPublishing={false} />);

    expect(screen.queryByTestId("agents-publish-step-auto_merge")).not.toBeInTheDocument();
  });

  it("describes a linked PR accurately when description publishing fails", () => {
    render(
      <PublishPipelineSteps
        status="description_failed"
        isPublishing={false}
        targetPullRequestLabel="PR #888"
      />,
    );

    expect(screen.getByText(/metadata outcome for PR #888/)).toBeInTheDocument();
    expect(screen.getByText(/could not confirm/i)).toBeInTheDocument();
    expect(screen.queryByText(/no pull request was opened/)).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-publish-step-describing")).toBeInTheDocument();
  });

  it("keeps the target-aware final step active while GitHub reconciliation is in progress", () => {
    const { container } = render(
      <PublishPipelineSteps
        status="pushing"
        isPublishing
        targetPullRequestLabel="PR #888"
        receiptPhase="reconciling"
        receiptState="unknown"
      />,
    );

    expect(screen.getByTestId("agents-publish-step-pushed")).toHaveTextContent(
      "Update PR",
    );
    expect(screen.getByText(/may have applied the description/i)).toBeInTheDocument();
    expect(container.querySelector(".animate-spin")).toBeInTheDocument();
  });
});
