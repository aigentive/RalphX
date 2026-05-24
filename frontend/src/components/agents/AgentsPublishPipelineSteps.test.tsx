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
});
