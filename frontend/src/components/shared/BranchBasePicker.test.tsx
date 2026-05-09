import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { BranchBasePicker } from "./BranchBasePicker";
import type { BranchBaseOption } from "./branchBaseOptions";

const options: BranchBaseOption[] = [
  {
    key: "project_default::main",
    label: "main",
    detail: "Project default",
    source: "project",
    selection: { kind: "project_default", ref: "main", displayName: "main" },
  },
  {
    key: "local_branch::feature/x",
    label: "feature/x",
    detail: "Local branch",
    source: "local",
    selection: { kind: "local_branch", ref: "feature/x", displayName: "feature/x" },
  },
];

describe("BranchBasePicker", () => {
  it("renders the placeholder when no value is selected", () => {
    render(
      <BranchBasePicker
        value=""
        onValueChange={vi.fn()}
        options={options}
        placeholder="Select base"
        testId="picker"
      />,
    );
    expect(screen.getByText("Select base")).toBeInTheDocument();
  });

  it("renders the selected option label", () => {
    render(
      <BranchBasePicker
        value="project_default::main"
        onValueChange={vi.fn()}
        options={options}
        placeholder="Select base"
        testId="picker"
      />,
    );
    expect(screen.getByText("main")).toBeInTheDocument();
  });

  it("opens the popover and lists filtered options", async () => {
    const user = userEvent.setup();
    const onValueChange = vi.fn();
    render(
      <BranchBasePicker
        value=""
        onValueChange={onValueChange}
        options={options}
        placeholder="Select base"
        testId="picker"
      />,
    );

    await user.click(screen.getByTestId("picker"));
    // Both options listed in popover.
    expect(screen.getAllByText("main").length).toBeGreaterThan(0);
    expect(screen.getByText("feature/x")).toBeInTheDocument();

    // Filter via search.
    const search = screen.getByPlaceholderText(/Search branches/i);
    fireEvent.change(search, { target: { value: "feature" } });
    expect(screen.queryByText("Project default")).toBeNull();
    expect(screen.getByText("feature/x")).toBeInTheDocument();

    await user.click(screen.getByText("feature/x"));
    expect(onValueChange).toHaveBeenCalledWith("local_branch::feature/x");
  });

  it("keeps cached options visible while a refresh is loading", async () => {
    const user = userEvent.setup();
    render(
      <BranchBasePicker
        value="local_branch::feature/x"
        onValueChange={vi.fn()}
        options={options}
        placeholder="Select base"
        testId="picker"
        isLoading
      />,
    );

    expect(screen.getByTestId("picker")).toHaveTextContent("feature/x");

    await user.click(screen.getByTestId("picker"));

    expect(screen.getByText("Refreshing branches...")).toBeInTheDocument();
    expect(screen.getAllByText("feature/x").length).toBeGreaterThan(0);
  });
});
