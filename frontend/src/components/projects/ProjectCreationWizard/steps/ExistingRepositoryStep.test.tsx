import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ProjectCandidate } from "@/types/project-candidate";
import { ExistingRepositoryStep } from "./ExistingRepositoryStep";
import { useUiStore } from "@/stores/uiStore";
import { useProjectStore } from "@/stores/projectStore";
import { projectsApi } from "@/api/projects";

vi.mock("@/api/projects", () => ({
  projectsApi: { validateWorktreeParent: vi.fn() },
}));

const defaultProps = {
  onCreate: vi.fn(),
  onClose: vi.fn(),
  onSwitchToCreateNew: vi.fn(),
  isCreating: false,
};

function renderStep(props: Partial<React.ComponentProps<typeof ExistingRepositoryStep>> = {}) {
  return render(<ExistingRepositoryStep {...defaultProps} {...props} />);
}

async function browseTo(user: ReturnType<typeof userEvent.setup>, path: string) {
  await user.click(screen.getByTestId("browse-button"));
  await waitFor(() => {
    expect(screen.getByTestId("folder-input")).toHaveValue(path);
  });
}

describe("ExistingRepositoryStep", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useUiStore.setState({ recentRepositories: [] });
    useProjectStore.setState({ projects: {}, activeProjectId: null });
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders the folder input immediately", () => {
    renderStep();
    expect(screen.getByTestId("folder-input")).toBeInTheDocument();
  });

  it("does not probe until the debounce elapses", async () => {
    vi.useFakeTimers();
    const onInspectCandidate = vi.fn().mockResolvedValue({ kind: "emptyDirectory" });
    const onBrowseFolder = vi.fn().mockResolvedValue("/Users/dev/my-app");
    render(
      <ExistingRepositoryStep
        {...defaultProps}
        onBrowseFolder={onBrowseFolder}
        onInspectCandidate={onInspectCandidate}
      />
    );

    await act(async () => {
      screen.getByTestId("browse-button").click();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(onInspectCandidate).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });
    expect(onInspectCandidate).toHaveBeenCalledWith("/Users/dev/my-app");
  });

  it("rejects a stale probe response superseded by a newer generation", async () => {
    vi.useFakeTimers();
    let resolveFirst: ((value: ProjectCandidate) => void) | undefined;
    let resolveSecond: ((value: ProjectCandidate) => void) | undefined;
    const onInspectCandidate = vi
      .fn()
      .mockImplementationOnce(
        () => new Promise<ProjectCandidate>((resolve) => (resolveFirst = resolve))
      )
      .mockImplementationOnce(
        () => new Promise<ProjectCandidate>((resolve) => (resolveSecond = resolve))
      );
    const onBrowseFolder = vi
      .fn()
      .mockResolvedValueOnce("/Users/dev/app-a")
      .mockResolvedValueOnce("/Users/dev/app-b");

    render(
      <ExistingRepositoryStep
        {...defaultProps}
        onBrowseFolder={onBrowseFolder}
        onInspectCandidate={onInspectCandidate}
      />
    );

    await act(async () => {
      screen.getByTestId("browse-button").click();
      await Promise.resolve();
      await Promise.resolve();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });
    expect(onInspectCandidate).toHaveBeenNthCalledWith(1, "/Users/dev/app-a");

    await act(async () => {
      screen.getByTestId("browse-button").click();
      await Promise.resolve();
      await Promise.resolve();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });
    expect(onInspectCandidate).toHaveBeenNthCalledWith(2, "/Users/dev/app-b");

    // Resolve the stale (first) probe after the newer one has already started.
    await act(async () => {
      resolveFirst?.({ kind: "emptyDirectory" });
      await Promise.resolve();
    });
    await act(async () => {
      resolveSecond?.({
        kind: "repository",
        repositoryRoot: "/Users/dev/app-b",
        currentBranch: "main",
        defaultBranch: "main",
        branches: ["main"],
        hasCommits: true,
        isDirty: false,
        capability: { kind: "localOnly" },
        alreadyRegisteredAs: null,
      });
      await Promise.resolve();
    });
    vi.useRealTimers();

    expect(screen.getByText(/Repository found/)).toBeInTheDocument();
  });

  it("degrades to continue when the probe fails", async () => {
    vi.useFakeTimers();
    const onInspectCandidate = vi.fn().mockRejectedValue(new Error("boom"));
    const onBrowseFolder = vi.fn().mockResolvedValue("/Users/dev/my-app");
    render(
      <ExistingRepositoryStep
        {...defaultProps}
        onBrowseFolder={onBrowseFolder}
        onInspectCandidate={onInspectCandidate}
      />
    );

    await act(async () => {
      screen.getByTestId("browse-button").click();
      await Promise.resolve();
      await Promise.resolve();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });
    vi.useRealTimers();

    expect(screen.getByText(/couldn't inspect this folder/i)).toBeInTheDocument();
    expect(screen.getByTestId("create-button")).not.toBeDisabled();
  });

  it("recovers non_empty_non_repo by switching to Create New with the path", async () => {
    const user = userEvent.setup();
    const onSwitchToCreateNew = vi.fn();
    const onInspectCandidate = vi
      .fn()
      .mockResolvedValue({ kind: "nonEmptyNonRepo", entryCount: 3 });
    const onBrowseFolder = vi.fn().mockResolvedValue("/Users/dev/my-app");
    renderStep({ onBrowseFolder, onInspectCandidate, onSwitchToCreateNew });

    await browseTo(user, "/Users/dev/my-app");
    await waitFor(() => {
      expect(screen.getByTestId("candidate-card")).toBeInTheDocument();
    });

    await user.click(screen.getByTestId("candidate-recovery-action"));
    expect(onSwitchToCreateNew).toHaveBeenCalledWith("/Users/dev/my-app");
  });

  it("recovers nested_in_repository by offering the repository root", async () => {
    const user = userEvent.setup();
    const onInspectCandidate = vi
      .fn()
      .mockResolvedValueOnce({
        kind: "nestedInRepository",
        repositoryRoot: "/Users/dev/repo-root",
      })
      .mockResolvedValueOnce({
        kind: "repository",
        repositoryRoot: "/Users/dev/repo-root",
        currentBranch: "main",
        defaultBranch: "main",
        branches: ["main"],
        hasCommits: true,
        isDirty: false,
        capability: { kind: "localOnly" },
        alreadyRegisteredAs: null,
      });
    const onBrowseFolder = vi.fn().mockResolvedValue("/Users/dev/repo-root/nested");
    renderStep({ onBrowseFolder, onInspectCandidate });

    await browseTo(user, "/Users/dev/repo-root/nested");
    await waitFor(() => {
      expect(screen.getByTestId("candidate-recovery-action")).toBeInTheDocument();
    });

    await user.click(screen.getByTestId("candidate-recovery-action"));
    await waitFor(() => {
      expect(screen.getByTestId("folder-input")).toHaveValue("/Users/dev/repo-root");
    });
  });

  it("blocks duplicate registration and offers to open the existing project", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    const onInspectCandidate = vi.fn().mockResolvedValue({
      kind: "repository",
      repositoryRoot: "/Users/dev/my-app",
      currentBranch: "main",
      defaultBranch: "main",
      branches: ["main"],
      hasCommits: true,
      isDirty: false,
      capability: { kind: "localOnly" },
      alreadyRegisteredAs: { id: "proj-1", name: "My App" },
    });
    const onBrowseFolder = vi.fn().mockResolvedValue("/Users/dev/my-app");
    const onCreate = vi.fn();
    renderStep({ onBrowseFolder, onInspectCandidate, onClose, onCreate });

    await browseTo(user, "/Users/dev/my-app");
    await waitFor(() => {
      expect(screen.getByText(/Open My App instead/)).toBeInTheDocument();
    });

    await user.click(screen.getByTestId("create-button"));
    expect(onCreate).not.toHaveBeenCalled();

    await user.click(screen.getByTestId("candidate-recovery-action"));
    expect(onClose).toHaveBeenCalled();
  });

  it("shows a display-only GitHub badge for github capability", async () => {
    const user = userEvent.setup();
    const onInspectCandidate = vi.fn().mockResolvedValue({
      kind: "repository",
      repositoryRoot: "/Users/dev/my-app",
      currentBranch: "main",
      defaultBranch: "main",
      branches: ["main"],
      hasCommits: true,
      isDirty: false,
      capability: { kind: "github", fetchUrl: "https://github.com/o/r.git", pushUrl: "https://github.com/o/r.git" },
      alreadyRegisteredAs: null,
    });
    const onBrowseFolder = vi.fn().mockResolvedValue("/Users/dev/my-app");
    renderStep({ onBrowseFolder, onInspectCandidate });

    await browseTo(user, "/Users/dev/my-app");
    await waitFor(() => {
      expect(screen.getByText(/GitHub pull requests available/)).toBeInTheDocument();
    });
  });

  it("populates base branch options from the probe instead of a hardcoded seed", async () => {
    const user = userEvent.setup();
    const onInspectCandidate = vi.fn().mockResolvedValue({
      kind: "repository",
      repositoryRoot: "/Users/dev/my-app",
      currentBranch: "develop",
      defaultBranch: "develop",
      branches: ["develop", "release"],
      hasCommits: true,
      isDirty: false,
      capability: { kind: "localOnly" },
      alreadyRegisteredAs: null,
    });
    const onBrowseFolder = vi.fn().mockResolvedValue("/Users/dev/my-app");
    renderStep({ onBrowseFolder, onInspectCandidate });

    await browseTo(user, "/Users/dev/my-app");
    await waitFor(() => {
      expect(screen.getByTestId("base-branch-select")).toHaveTextContent("develop");
    });
  });

  describe("recent repositories", () => {
    it("shows recents before a folder is selected, and hides them once one is", async () => {
      const user = userEvent.setup();
      useUiStore.setState({
        recentRepositories: [
          { path: "/Users/dev/recent-app", name: "recent-app", lastUsedAt: "2026-08-01T00:00:00Z" },
        ],
      });
      const onInspectCandidate = vi.fn().mockResolvedValue({ kind: "emptyDirectory" });
      renderStep({ onInspectCandidate });

      expect(screen.getByTestId("recent-repositories-list")).toBeInTheDocument();

      await user.click(screen.getByTestId("recent-repository-item"));
      expect(screen.getByTestId("folder-input")).toHaveValue("/Users/dev/recent-app");
      expect(screen.queryByTestId("recent-repositories-list")).not.toBeInTheDocument();
    });

    it("hides a recent entry whose path already belongs to a registered project", () => {
      useProjectStore.setState({
        projects: {
          "proj-1": {
            id: "proj-1",
            name: "Existing",
            workingDirectory: "/Users/dev/registered-app",
            baseBranch: "main",
            gitMode: "worktree",
            worktreeParentDirectory: "~/ralphx-worktrees",
            useFeatureBranches: true,
            mergeValidationMode: "block",
            detectedAnalysis: null,
            customAnalysis: null,
            analyzedAt: null,
            githubPrEnabled: false,
            createdAt: "2026-01-01T00:00:00Z",
            updatedAt: "2026-01-01T00:00:00Z",
          },
        },
        activeProjectId: null,
      });
      useUiStore.setState({
        recentRepositories: [
          { path: "/Users/dev/registered-app", name: "registered-app", lastUsedAt: "2026-08-01T00:00:00Z" },
        ],
      });
      renderStep();

      expect(screen.queryByTestId("recent-repositories-list")).not.toBeInTheDocument();
    });

    it("selecting a recent entry that has moved feeds the normal probe path and degrades to the error card", async () => {
      const user = userEvent.setup();
      useUiStore.setState({
        recentRepositories: [
          { path: "/Users/dev/moved-app", name: "moved-app", lastUsedAt: "2026-08-01T00:00:00Z" },
        ],
      });
      const onCreate = vi.fn();
      const onInspectCandidate = vi.fn().mockResolvedValue({ kind: "notFound" });
      renderStep({ onInspectCandidate, onCreate });

      await user.click(screen.getByTestId("recent-repository-item"));
      await waitFor(() => expect(onInspectCandidate).toHaveBeenCalledWith("/Users/dev/moved-app"));
      await waitFor(() => {
        expect(screen.getByText(/doesn't exist yet/i)).toBeInTheDocument();
      });

      await user.click(screen.getByTestId("create-button"));
      expect(onCreate).not.toHaveBeenCalled();
    });

    it("records a successfully-created repository as recent", async () => {
      const user = userEvent.setup();
      const onCreate = vi.fn().mockResolvedValue(undefined);
      const onInspectCandidate = vi.fn().mockResolvedValue({
        kind: "repository",
        repositoryRoot: "/Users/dev/new-app",
        currentBranch: "main",
        defaultBranch: "main",
        branches: ["main"],
        hasCommits: true,
        isDirty: false,
        capability: { kind: "localOnly" },
        alreadyRegisteredAs: null,
      });
      const onBrowseFolder = vi.fn().mockResolvedValue("/Users/dev/new-app");
      renderStep({ onCreate, onInspectCandidate, onBrowseFolder });

      await browseTo(user, "/Users/dev/new-app");
      await waitFor(() => {
        expect(screen.getByTestId("base-branch-select")).toHaveTextContent("main");
      });
      await user.click(screen.getByTestId("create-button"));

      await waitFor(() => {
        expect(useUiStore.getState().recentRepositories).toEqual([
          expect.objectContaining({ path: "/Users/dev/new-app", name: "new-app" }),
        ]);
      });
    });
  });

  describe("worktree-parent verdict blocking", () => {
    it("blocks Create when the worktree-parent verdict is blocking", async () => {
      const user = userEvent.setup();
      vi.mocked(projectsApi.validateWorktreeParent).mockResolvedValue({
        verdict: "insideRepository",
        path: "/Users/dev/my-app/worktrees",
      });
      const onInspectCandidate = vi.fn().mockResolvedValue({
        kind: "repository",
        repositoryRoot: "/Users/dev/my-app",
        currentBranch: "main",
        defaultBranch: "main",
        branches: ["main"],
        hasCommits: true,
        isDirty: false,
        capability: { kind: "localOnly" },
        alreadyRegisteredAs: null,
      });
      const onBrowseFolder = vi.fn().mockResolvedValue("/Users/dev/my-app");
      const onCreate = vi.fn();
      renderStep({ onBrowseFolder, onInspectCandidate, onCreate });

      await browseTo(user, "/Users/dev/my-app");
      await user.click(screen.getByTestId("advanced-settings-trigger"));
      await user.type(
        screen.getByTestId("worktree-parent-input"),
        "/Users/dev/my-app/worktrees"
      );

      await waitFor(() => {
        expect(screen.getByTestId("worktree-parent-verdict")).toHaveTextContent(
          /inside the repository/i
        );
      });

      await user.click(screen.getByTestId("create-button"));
      expect(onCreate).not.toHaveBeenCalled();
    });
  });
});
