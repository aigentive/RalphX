import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NewRepositoryStep } from "./NewRepositoryStep";

const defaultProps = {
  onCreate: vi.fn(),
  onClose: vi.fn(),
  isCreating: false,
};

function renderStep(props: Partial<React.ComponentProps<typeof NewRepositoryStep>> = {}) {
  return render(<NewRepositoryStep {...defaultProps} {...props} />);
}

async function browseParentTo(user: ReturnType<typeof userEvent.setup>, path: string) {
  await user.click(screen.getByTestId("browse-parent-button"));
  await waitFor(() => {
    expect(screen.getByTestId("new-project-parent-input")).toHaveValue(path);
  });
}

describe("NewRepositoryStep", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("defaults the starting branch to main as free text", () => {
    renderStep();
    expect(screen.getByTestId("base-branch-select")).toHaveValue("main");
  });

  it("prepares the destination then creates the project with the resolved path", async () => {
    const user = userEvent.setup();
    const onPrepareDirectory = vi.fn().mockResolvedValue({ path: "/parent/my-app", created: true });
    const onCreate = vi.fn().mockResolvedValue(undefined);
    const onBrowseFolder = vi.fn().mockResolvedValue("/parent");
    renderStep({ onPrepareDirectory, onCreate, onBrowseFolder });

    await browseParentTo(user, "/parent");
    await user.type(screen.getByTestId("new-project-folder-name-input"), "my-app");
    await user.click(screen.getByTestId("create-button"));

    await waitFor(() => {
      expect(onPrepareDirectory).toHaveBeenCalledWith({
        parentDirectory: "/parent",
        folderName: "my-app",
      });
    });
    await waitFor(() => {
      expect(onCreate).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "my-app",
          workingDirectory: "/parent/my-app",
          baseBranch: "main",
        })
      );
    });
  });

  it("rolls back the prepared directory when create_project fails after RalphX created it", async () => {
    const user = userEvent.setup();
    const onPrepareDirectory = vi.fn().mockResolvedValue({ path: "/parent/my-app", created: true });
    const onCreate = vi.fn().mockRejectedValue(new Error("boom"));
    const onDiscardDirectory = vi.fn().mockResolvedValue(undefined);
    const onBrowseFolder = vi.fn().mockResolvedValue("/parent");
    renderStep({ onPrepareDirectory, onCreate, onDiscardDirectory, onBrowseFolder });

    await browseParentTo(user, "/parent");
    await user.type(screen.getByTestId("new-project-folder-name-input"), "my-app");
    await user.click(screen.getByTestId("create-button"));

    await waitFor(() => {
      expect(onDiscardDirectory).toHaveBeenCalledWith("/parent/my-app");
    });
  });

  it("does not roll back when the directory already existed (created: false)", async () => {
    const user = userEvent.setup();
    const onPrepareDirectory = vi.fn().mockResolvedValue({ path: "/parent/my-app", created: false });
    const onCreate = vi.fn().mockRejectedValue(new Error("boom"));
    const onDiscardDirectory = vi.fn().mockResolvedValue(undefined);
    const onBrowseFolder = vi.fn().mockResolvedValue("/parent");
    renderStep({ onPrepareDirectory, onCreate, onDiscardDirectory, onBrowseFolder });

    await browseParentTo(user, "/parent");
    await user.type(screen.getByTestId("new-project-folder-name-input"), "my-app");
    await user.click(screen.getByTestId("create-button"));

    await waitFor(() => {
      expect(onCreate).toHaveBeenCalled();
    });
    expect(onDiscardDirectory).not.toHaveBeenCalled();
  });

  it("surfaces the App-level error in wizard-error", () => {
    renderStep({ error: "Failed to create project" });
    expect(screen.getByTestId("wizard-error")).toHaveTextContent("Failed to create project");
  });

  it("requires parent folder and folder name before submitting", async () => {
    const user = userEvent.setup();
    const onPrepareDirectory = vi.fn();
    renderStep({ onPrepareDirectory });

    await user.click(screen.getByTestId("create-button"));

    expect(onPrepareDirectory).not.toHaveBeenCalled();
    expect(screen.getByTestId("new-project-parent-input-error")).toBeInTheDocument();
    expect(screen.getByTestId("new-project-folder-name-input-error")).toBeInTheDocument();
  });
});
