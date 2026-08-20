import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { CloneAdvancedOptions, type CloneAdvancedOptionsProps } from "./CloneAdvancedOptions";

const defaultProps: CloneAdvancedOptionsProps = {
  depth: "",
  onDepthChange: vi.fn(),
  singleBranch: false,
  onSingleBranchChange: vi.fn(),
  recurseSubmodules: false,
  onRecurseSubmodulesChange: vi.fn(),
  isCreating: false,
  open: true,
  onOpenChange: vi.fn(),
};

function renderOptions(props: Partial<CloneAdvancedOptionsProps> = {}) {
  return render(<CloneAdvancedOptions {...defaultProps} {...props} />);
}

describe("CloneAdvancedOptions", () => {
  it("renders plain-language labels only - no git flags on screen", () => {
    renderOptions();
    expect(screen.queryByText(/--depth/)).not.toBeInTheDocument();
    expect(screen.queryByText(/--single-branch/)).not.toBeInTheDocument();
    expect(screen.queryByText(/--recurse-submodules/)).not.toBeInTheDocument();
    expect(screen.getByTestId("clone-depth-input")).toBeInTheDocument();
    expect(screen.getByTestId("clone-single-branch-toggle")).toBeInTheDocument();
    expect(screen.getByTestId("clone-submodules-toggle")).toBeInTheDocument();
  });

  it("reports depth changes", async () => {
    const user = userEvent.setup();
    const onDepthChange = vi.fn();
    renderOptions({ onDepthChange });

    await user.type(screen.getByTestId("clone-depth-input"), "1");
    expect(onDepthChange).toHaveBeenCalledWith("1");
  });

  it("toggles single-branch and submodules", async () => {
    const user = userEvent.setup();
    const onSingleBranchChange = vi.fn();
    const onRecurseSubmodulesChange = vi.fn();
    renderOptions({ onSingleBranchChange, onRecurseSubmodulesChange });

    await user.click(screen.getByTestId("clone-single-branch-toggle"));
    expect(onSingleBranchChange).toHaveBeenCalledWith(true);

    await user.click(screen.getByTestId("clone-submodules-toggle"));
    expect(onRecurseSubmodulesChange).toHaveBeenCalledWith(true);
  });
});
