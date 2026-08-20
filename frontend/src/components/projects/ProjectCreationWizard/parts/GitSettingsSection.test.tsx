import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, act } from "@testing-library/react";
import { GitSettingsSection, type GitSettingsSectionProps } from "./GitSettingsSection";
import { projectsApi } from "@/api/projects";
import type { WorktreeParentVerdict } from "@/types/worktree-parent";

vi.mock("@/api/projects", () => ({
  projectsApi: { validateWorktreeParent: vi.fn() },
}));

const mockValidateWorktreeParent = vi.mocked(projectsApi.validateWorktreeParent);

const defaultProps: GitSettingsSectionProps = {
  baseBranchMode: "select",
  baseBranchLabel: "Base branch",
  baseBranch: "main",
  onBaseBranchChange: vi.fn(),
  branches: ["main"],
  worktreePath: "/tmp/worktrees/app/task-1",
  worktreeParentDirectory: "",
  onWorktreeParentDirectoryChange: vi.fn(),
  showAdvanced: true,
  onShowAdvancedChange: vi.fn(),
  isCreating: false,
};

function renderSection(props: Partial<GitSettingsSectionProps> = {}) {
  return render(<GitSettingsSection {...defaultProps} {...props} />);
}

async function advanceDebounce() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(300);
  });
}

describe("GitSettingsSection - worktree parent verdict", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("does not check an empty worktree parent", () => {
    renderSection();
    expect(mockValidateWorktreeParent).not.toHaveBeenCalled();
  });

  it("renders the ok verdict without blocking", async () => {
    mockValidateWorktreeParent.mockResolvedValue({
      verdict: "ok",
      path: "/Users/dev/worktrees",
    });
    const onBlockingChange = vi.fn();
    renderSection({
      worktreeParentDirectory: "/Users/dev/worktrees",
      onWorktreeParentBlockingChange: onBlockingChange,
    });

    await advanceDebounce();

    expect(screen.getByTestId("worktree-parent-verdict")).toHaveTextContent(/ready to use/i);
    expect(onBlockingChange).toHaveBeenLastCalledWith(false);
  });

  it.each<[WorktreeParentVerdict, RegExp]>([
    [{ verdict: "notFound", path: "/x" }, /doesn't exist yet/i],
    [{ verdict: "notADirectory", path: "/x" }, /isn't a folder/i],
    [{ verdict: "insideRepository", path: "/x" }, /inside the repository/i],
    [{ verdict: "invalid", message: "Enter a folder path." }, /enter a folder path/i],
  ])("blocks Create for verdict %o", async (verdict, expectedText) => {
    mockValidateWorktreeParent.mockResolvedValue(verdict);
    const onBlockingChange = vi.fn();
    renderSection({
      worktreeParentDirectory: "/x",
      onWorktreeParentBlockingChange: onBlockingChange,
    });

    await advanceDebounce();

    expect(screen.getByTestId("worktree-parent-verdict")).toHaveTextContent(expectedText);
    expect(onBlockingChange).toHaveBeenLastCalledWith(true);
  });

  it("warns without blocking for not_writable (proof obligation 14)", async () => {
    mockValidateWorktreeParent.mockResolvedValue({ verdict: "notWritable", path: "/readonly" });
    const onBlockingChange = vi.fn();
    renderSection({
      worktreeParentDirectory: "/readonly",
      onWorktreeParentBlockingChange: onBlockingChange,
    });

    await advanceDebounce();

    expect(screen.getByTestId("worktree-parent-verdict")).toHaveTextContent(/may not be able to write/i);
    expect(onBlockingChange).toHaveBeenLastCalledWith(false);
  });

  it("checks tilde-expanded paths the same as any other path", async () => {
    mockValidateWorktreeParent.mockResolvedValue({ verdict: "ok", path: "~/ralphx-worktrees" });
    renderSection({ worktreeParentDirectory: "~/ralphx-worktrees" });

    await advanceDebounce();

    expect(mockValidateWorktreeParent).toHaveBeenCalledWith({ path: "~/ralphx-worktrees" });
  });

  it("passes the repository root so the backend can detect containment", async () => {
    mockValidateWorktreeParent.mockResolvedValue({ verdict: "ok", path: "/x" });
    renderSection({
      worktreeParentDirectory: "/x",
      worktreeParentRepositoryRoot: "/Users/dev/my-repo",
    });

    await advanceDebounce();

    expect(mockValidateWorktreeParent).toHaveBeenCalledWith({
      path: "/x",
      repositoryRoot: "/Users/dev/my-repo",
    });
  });

  it("renders a Browse button only when onBrowseWorktreeParent is provided", () => {
    renderSection();
    expect(screen.queryByTestId("worktree-parent-browse-button")).not.toBeInTheDocument();

    renderSection({ onBrowseWorktreeParent: vi.fn() });
    expect(screen.getByTestId("worktree-parent-browse-button")).toBeInTheDocument();
  });
});
