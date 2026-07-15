import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ArtifactLoadingState } from "./AgentsArtifactEmptyState";

describe("ArtifactLoadingState", () => {
  it("renders a visible status loading state", () => {
    render(<ArtifactLoadingState title="Loading pull request..." />);

    const status = screen.getByRole("status", { name: "Loading pull request..." });
    const lines = screen.getAllByTestId("agents-artifact-loading-line");

    expect(status).toHaveTextContent("Loading pull request...");
    expect(lines).toHaveLength(3);
    expect(lines[0]).toHaveStyle("background-color: var(--bg-hover)");
  });
});
