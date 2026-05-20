import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { PublishPipelineSteps } from "./AgentsPublishPipelineSteps";

describe("PublishPipelineSteps", () => {
  it("shows auto-merge as a non-blocking post-publish step when requested", () => {
    render(
      <PublishPipelineSteps
        status="pushed"
        isPublishing={false}
        autoMergeDesired
        autoMergeCurrent={false}
        prSupervisionStatus="waiting"
      />,
    );

    expect(screen.getByTestId("agents-publish-step-auto_merge")).toHaveTextContent(
      "Request auto-merge",
    );
    expect(screen.queryByText(/latest publish attempt failed/i)).not.toBeInTheDocument();
  });

  it("omits the auto-merge step when auto-merge was not requested", () => {
    render(<PublishPipelineSteps status="pushed" isPublishing={false} />);

    expect(screen.queryByTestId("agents-publish-step-auto_merge")).not.toBeInTheDocument();
  });
});
