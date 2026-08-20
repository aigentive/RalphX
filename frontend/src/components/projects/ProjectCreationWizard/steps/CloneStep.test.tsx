/**
 * Tests for CloneStep.
 *
 * useCloneJob and the api module are mocked rather than driving real IPC.
 * The useCloneJob mock is a small real React hook (useState-backed) so
 * job.status transitions naturally re-render the component the same way the
 * real hook does, without touching EventProvider/EventBus.
 *
 * These tests run under fake timers (debounced validate_clone_target), so
 * interactions use raw DOM events wrapped in `act` rather than userEvent -
 * userEvent's internal scheduling does not play well with fake timers (see
 * ExistingRepositoryStep.test.tsx for the same convention in this directory).
 */

import { useCallback, useState, type ComponentProps } from "react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { CloneStep } from "./CloneStep";
import { projectsApi, type CloneTargetPlan, type StartProjectCloneInput } from "@/api/projects";
import type { CloneJobStatus } from "@/types/clone";
import { createTestQueryClient } from "@/test/store-utils";

const mockCloneJob = vi.hoisted(() => ({
  start: vi.fn(),
  cancel: vi.fn(),
  pendingStatus: null as CloneJobStatus | null,
  startResult: true as boolean,
  startError: null as string | null,
}));

const mockAuth = vi.hoisted(() => ({
  isAuthenticated: false,
  login: vi.fn(),
}));

vi.mock("@/hooks/useCloneJob", () => ({
  useCloneJob: () => {
    const [status, setStatus] = useState<CloneJobStatus | null>(null);

    const [startError, setStartError] = useState<string | null>(null);

    const start = useCallback(async (input: StartProjectCloneInput) => {
      mockCloneJob.start(input);
      setStatus(null);
      if (!mockCloneJob.startResult) {
        setStartError(mockCloneJob.startError);
        return false;
      }
      setStartError(null);
      if (mockCloneJob.pendingStatus) {
        setStatus(mockCloneJob.pendingStatus);
      }
      return true;
    }, []);

    const cancel = useCallback(async () => {
      mockCloneJob.cancel();
      setStatus({ state: "cancelled", cleanedUp: true });
    }, []);

    const error =
      status?.state === "unknown"
        ? "We lost track of this clone. Try again."
        : status?.state === "failed"
          ? status.message
          : startError;

    return {
      phase: status === null ? "receiving" : null,
      percent: null,
      received: null,
      total: null,
      lines: ["Cloning into 'r'..."],
      status,
      error,
      cleanedUp: status?.state === "failed" || status?.state === "cancelled" ? status.cleanedUp : null,
      start,
      cancel,
      reset: vi.fn(),
    };
  },
}));

vi.mock("@/hooks/useGithubSettings", () => ({
  useGhAuthStatus: () => ({ data: mockAuth.isAuthenticated, isLoading: false }),
  useLoginGhWithBrowser: () => ({ mutate: mockAuth.login, isPending: false }),
}));

vi.mock("@/api/projects", () => ({
  projectsApi: {
    validateCloneTarget: vi.fn(),
    listGithubRepositories: vi.fn().mockResolvedValue([]),
    validateWorktreeParent: vi.fn(),
  },
}));

const mockValidateCloneTarget = vi.mocked(projectsApi.validateCloneTarget);

const READY_PLAN: CloneTargetPlan = {
  normalizedUrl: "https://github.com/o/r.git",
  folderName: "r",
  branch: null,
  suggestedSshUrl: "git@github.com:o/r.git",
  destination: "/tmp/dest/r",
  ready: true,
  problem: null,
};

function renderCloneStep(props: Partial<ComponentProps<typeof CloneStep>> = {}) {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <CloneStep
        onCreate={vi.fn()}
        onClose={vi.fn()}
        isCreating={false}
        onBrowseFolder={vi.fn().mockResolvedValue("/tmp/dest")}
        {...props}
      />
    </QueryClientProvider>
  );
}

async function clickAndFlush(element: HTMLElement) {
  await act(async () => {
    element.click();
    await Promise.resolve();
    await Promise.resolve();
  });
}

async function typeUrl(value: string) {
  await act(async () => {
    fireEvent.change(screen.getByTestId("clone-url-input"), { target: { value } });
  });
}

async function advanceDebounce() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(300);
  });
}

async function fillReadyForm() {
  await typeUrl("https://github.com/o/r.git");
  await clickAndFlush(screen.getByTestId("browse-parent-button"));
  await advanceDebounce();
}

describe("CloneStep", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockCloneJob.pendingStatus = null;
    mockCloneJob.startResult = true;
    mockCloneJob.startError = null;
    mockAuth.isAuthenticated = false;
    mockValidateCloneTarget.mockResolvedValue(READY_PLAN);
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("does not call validate_clone_target for an empty URL", () => {
    renderCloneStep();
    expect(mockValidateCloneTarget).not.toHaveBeenCalled();
  });

  it("keeps the primary action disabled until the plan is ready, then enables it", async () => {
    renderCloneStep();

    expect(screen.getByTestId("clone-start-button")).toBeDisabled();

    await fillReadyForm();

    expect(mockValidateCloneTarget).toHaveBeenCalledWith(
      expect.objectContaining({ url: "https://github.com/o/r.git", parentDirectory: "/tmp/dest" })
    );
    expect(screen.getByTestId("clone-start-button")).not.toBeDisabled();
  });

  it("rejects a superseded validation response via the generation counter", async () => {
    let resolveStale: ((plan: CloneTargetPlan) => void) | null = null;
    const stalePromise = new Promise<CloneTargetPlan>((resolve) => {
      resolveStale = resolve;
    });
    mockValidateCloneTarget.mockImplementationOnce(() => stalePromise);
    mockValidateCloneTarget.mockImplementationOnce(() =>
      Promise.resolve({ ...READY_PLAN, normalizedUrl: "https://github.com/o/second.git" })
    );

    renderCloneStep();

    await typeUrl("https://github.com/o/first.git");
    await advanceDebounce();

    await typeUrl("https://github.com/o/second.git");
    await clickAndFlush(screen.getByTestId("browse-parent-button"));
    await advanceDebounce();

    expect(screen.getByTestId("clone-start-button")).not.toBeDisabled();

    // The stale first request now resolves with a not-ready plan; it must be
    // discarded since a newer generation already won.
    await act(async () => {
      resolveStale?.({ ...READY_PLAN, ready: false, problem: "stale response" });
      await Promise.resolve();
    });

    expect(screen.getByTestId("clone-start-button")).not.toBeDisabled();
    expect(screen.queryByText("stale response")).not.toBeInTheDocument();
  });

  it("returns to configure with an error banner when start_project_clone rejects", async () => {
    mockCloneJob.startResult = false;
    mockCloneJob.startError = "Folder name cannot contain '/'";
    const onActiveChange = vi.fn();
    renderCloneStep({ onActiveChange });
    await fillReadyForm();

    await clickAndFlush(screen.getByTestId("clone-start-button"));

    expect(screen.getByTestId("clone-url-input")).toBeInTheDocument();
    expect(screen.queryByTestId("clone-progress")).not.toBeInTheDocument();
    expect(screen.getByText("Folder name cannot contain '/'")).toBeInTheDocument();
    expect(onActiveChange).toHaveBeenLastCalledWith(false);
  });

  it("wires the cancel button through to useCloneJob.cancel", async () => {
    renderCloneStep();
    await fillReadyForm();

    await clickAndFlush(screen.getByTestId("clone-start-button"));
    expect(screen.getByTestId("clone-progress")).toBeInTheDocument();

    await clickAndFlush(screen.getByTestId("clone-cancel-button"));
    expect(mockCloneJob.cancel).toHaveBeenCalledTimes(1);
  });

  it("renders an auth-failure card and applies the suggested SSH URL without a project id", async () => {
    mockCloneJob.pendingStatus = {
      state: "failed",
      code: "CLONE_AUTH_FAILED",
      message: "fatal: Authentication failed",
      cleanedUp: true,
    };
    renderCloneStep();
    await fillReadyForm();

    await clickAndFlush(screen.getByTestId("clone-start-button"));

    expect(screen.getByTestId("clone-auth-card")).toBeInTheDocument();
    // Plain-language only: the raw backend message is never rendered.
    expect(screen.queryByText(/fatal:/)).not.toBeInTheDocument();
    expect(screen.getByTestId("clone-auth-login-button")).toBeInTheDocument();

    await clickAndFlush(screen.getByTestId("clone-use-ssh-button"));
    expect(screen.getByTestId("clone-url-input")).toHaveValue("git@github.com:o/r.git");
  });

  it("advances a completed clone to the prefilled settings step and finishes through onCreate", async () => {
    const onCreate = vi.fn();
    mockCloneJob.pendingStatus = {
      state: "completed",
      destination: "/tmp/dest/my-repo",
      defaultBranch: "develop",
      capability: { kind: "localOnly" },
    };
    renderCloneStep({ onCreate });
    await fillReadyForm();

    await clickAndFlush(screen.getByTestId("clone-start-button"));

    expect(screen.getByTestId("clone-settings-destination")).toHaveTextContent("/tmp/dest/my-repo");
    expect(screen.getByTestId("project-name-input")).toHaveValue("my-repo");
    expect(screen.getByTestId("base-branch-select")).toHaveValue("develop");

    await clickAndFlush(screen.getByTestId("create-button"));

    expect(onCreate).toHaveBeenCalledWith({
      name: "my-repo",
      workingDirectory: "/tmp/dest/my-repo",
      gitMode: "worktree",
      baseBranch: "develop",
      worktreeParentDirectory: "~/ralphx-worktrees",
    });
  });

  it("sends no advanced-option flags when the defaults are unchanged", async () => {
    renderCloneStep();
    await fillReadyForm();

    await clickAndFlush(screen.getByTestId("clone-start-button"));

    expect(mockCloneJob.start).toHaveBeenCalledTimes(1);
    const input = mockCloneJob.start.mock.calls[0]?.[0] as StartProjectCloneInput;
    expect(input).not.toHaveProperty("depth");
    expect(input).not.toHaveProperty("singleBranch");
    expect(input).not.toHaveProperty("recurseSubmodules");
  });

  it("maps advanced clone options to startProjectClone arguments", async () => {
    renderCloneStep();
    await fillReadyForm();

    await clickAndFlush(screen.getByTestId("clone-advanced-options-trigger"));
    await act(async () => {
      fireEvent.change(screen.getByTestId("clone-depth-input"), { target: { value: "1" } });
    });
    await clickAndFlush(screen.getByTestId("clone-single-branch-toggle"));
    await clickAndFlush(screen.getByTestId("clone-submodules-toggle"));

    await clickAndFlush(screen.getByTestId("clone-start-button"));

    expect(mockCloneJob.start).toHaveBeenCalledWith(
      expect.objectContaining({ depth: 1, singleBranch: true, recurseSubmodules: true })
    );
  });

  it("hides the GitHub repo picker when unauthenticated", () => {
    renderCloneStep();
    expect(screen.queryByTestId("github-repo-picker-trigger")).not.toBeInTheDocument();
  });

  it("shows the GitHub repo picker trigger when authenticated", () => {
    mockAuth.isAuthenticated = true;
    renderCloneStep();
    expect(screen.getByTestId("github-repo-picker-trigger")).toBeInTheDocument();
  });

  it("fills the URL when a repo is selected from the picker", async () => {
    mockAuth.isAuthenticated = true;
    vi.mocked(projectsApi.listGithubRepositories).mockResolvedValue([
      { nameWithOwner: "acme/demo", description: null, isPrivate: false, updatedAt: null },
    ]);
    renderCloneStep();

    // Exercise handleSelectRepo directly via the picker's onSelectRepo prop
    // wiring rather than the fetch pipeline - GitHubRepoPicker.test.tsx
    // already proves the fetch-on-expand and selection behavior in isolation.
    await clickAndFlush(screen.getByTestId("github-repo-picker-trigger"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    await clickAndFlush(screen.getByTestId("github-repo-picker-item"));

    expect(screen.getByTestId("clone-url-input")).toHaveValue("acme/demo");
  });
});
