import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { renderHook } from "@testing-library/react";
import { act } from "react";

import type { Project } from "@/types/project";

// ---------------------------------------------------------------------------
// Mocks (declared before importing the hook)
// ---------------------------------------------------------------------------

const {
  toastFn,
  toastDismiss,
  mockOpenModal,
  mockResumeMutate,
  mocks,
} = vi.hoisted(() => {
  const mocks = {
    activeProject: null as Project | null,
    diagnostics: { data: undefined, isLoading: false, isError: false } as {
      data?: unknown;
      isLoading?: boolean;
      isError?: boolean;
    },
    ghAuth: { data: undefined, isLoading: false } as {
      data?: unknown;
      isLoading?: boolean;
    },
  };
  return {
    toastFn: vi.fn(),
    toastDismiss: vi.fn(),
    mockOpenModal: vi.fn(),
    mockResumeMutate: vi.fn(),
    mocks,
  };
});

vi.mock("sonner", () => ({
  toast: Object.assign(toastFn, { dismiss: toastDismiss }),
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: (selector: (s: { openModal: typeof mockOpenModal }) => unknown) =>
    selector({ openModal: mockOpenModal }),
}));

vi.mock("@/stores/projectStore", () => ({
  selectActiveProject: () => mocks.activeProject,
  useProjectStore: (selector: (s: unknown) => unknown) => selector(undefined),
}));

vi.mock("@/hooks/useGithubSettings", () => ({
  useGitAuthDiagnostics: () => mocks.diagnostics,
  useGhAuthStatus: () => mocks.ghAuth,
  useResumeDeferredGitStartup: () => ({ mutate: mockResumeMutate }),
}));

import { useGitAuthStartupNotification } from "./useGitAuthStartupNotification";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeProject(overrides: Partial<Project> = {}): Project {
  return {
    id: "project-1",
    name: "RalphX",
    workingDirectory: "/repo",
    gitMode: "worktree",
    baseBranch: "main",
    worktreeParentDirectory: null,
    useFeatureBranches: true,
    mergeValidationMode: "block",
    detectedAnalysis: null,
    customAnalysis: null,
    analyzedAt: null,
    githubPrEnabled: true,
    createdAt: "2026-05-01T00:00:00Z",
    updatedAt: "2026-05-01T00:00:00Z",
    ...overrides,
  };
}

function diagnosticsHttps() {
  return {
    fetchUrl: "https://github.com/owner/repo.git",
    pushUrl: "https://github.com/owner/repo.git",
    fetchKind: "HTTPS",
    pushKind: "HTTPS",
    mixedAuthModes: false,
    canSwitchToSsh: true,
    suggestedSshUrl: "git@github.com:owner/repo.git",
  };
}

function diagnosticsSsh() {
  return {
    fetchUrl: "git@github.com:owner/repo.git",
    pushUrl: "git@github.com:owner/repo.git",
    fetchKind: "SSH",
    pushKind: "SSH",
    mixedAuthModes: false,
    canSwitchToSsh: false,
    suggestedSshUrl: null,
  };
}

// ---------------------------------------------------------------------------

describe("useGitAuthStartupNotification (hook)", () => {
  beforeEach(() => {
    toastFn.mockClear();
    toastDismiss.mockClear();
    mockOpenModal.mockClear();
    mockResumeMutate.mockClear();
    mocks.activeProject = null;
    mocks.diagnostics = { data: undefined, isLoading: false, isError: false };
    mocks.ghAuth = { data: undefined, isLoading: false };
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("does nothing when there is no active project", () => {
    renderHook(() => useGitAuthStartupNotification());
    expect(toastFn).not.toHaveBeenCalled();
  });

  it("emits the warning toast and Open Settings opens the repository modal", () => {
    mocks.activeProject = makeProject();
    mocks.diagnostics = { data: diagnosticsHttps(), isLoading: false, isError: false };
    mocks.ghAuth = { data: false, isLoading: false };

    renderHook(() => useGitAuthStartupNotification());
    expect(toastFn).toHaveBeenCalledTimes(1);

    // Render the toast body to invoke the Open Settings click handler.
    const [body] = toastFn.mock.calls[0]!;
    // Walk the createElement tree to the action button props.
    interface Node {
      type?: unknown;
      props?: { children?: unknown[]; onClick?: () => void };
    }
    const root = body as Node;
    const buttonsRow = (root.props?.children as Node[] | undefined)?.[2] as Node | undefined;
    const buttons = (buttonsRow?.props?.children as Node[] | undefined) ?? [];
    const openBtn = buttons[0];
    const laterBtn = buttons[1];
    expect(openBtn?.props?.onClick).toBeDefined();
    openBtn!.props!.onClick!();
    expect(mockOpenModal).toHaveBeenCalledWith("settings", { section: "repository" });
    expect(toastDismiss).toHaveBeenCalledWith("git-auth-startup:project-1");

    laterBtn!.props!.onClick!();
    expect(toastDismiss).toHaveBeenCalledWith("git-auth-startup:project-1");
    expect(toastDismiss).toHaveBeenCalledTimes(2);
  });

  it("does not re-emit when the notification key is unchanged on re-render", () => {
    mocks.activeProject = makeProject();
    mocks.diagnostics = { data: diagnosticsHttps(), isLoading: false, isError: false };
    mocks.ghAuth = { data: false, isLoading: false };

    const { rerender } = renderHook(() => useGitAuthStartupNotification());
    rerender();
    rerender();
    expect(toastFn).toHaveBeenCalledTimes(1);
  });

  it("resumes deferred startup when a previously blocked project becomes healthy", () => {
    mocks.activeProject = makeProject();
    mocks.diagnostics = { data: diagnosticsHttps(), isLoading: false, isError: false };
    mocks.ghAuth = { data: false, isLoading: false };

    const { rerender } = renderHook(() => useGitAuthStartupNotification());
    expect(toastFn).toHaveBeenCalledTimes(1);

    // Project recovers — gh now authenticated, fetch/push back on SSH.
    act(() => {
      mocks.diagnostics = { data: diagnosticsSsh(), isLoading: false, isError: false };
      mocks.ghAuth = { data: true, isLoading: false };
    });
    rerender();
    expect(mockResumeMutate).toHaveBeenCalledTimes(1);
  });

  it("waits for diagnostics to finish loading before emitting", () => {
    mocks.activeProject = makeProject();
    mocks.diagnostics = { data: undefined, isLoading: true, isError: false };
    mocks.ghAuth = { data: undefined, isLoading: true };
    renderHook(() => useGitAuthStartupNotification());
    expect(toastFn).not.toHaveBeenCalled();
  });

  it("emits when diagnostics fail (isError) so the user is still warned", () => {
    mocks.activeProject = makeProject();
    mocks.diagnostics = { data: undefined, isLoading: false, isError: true };
    mocks.ghAuth = { data: false, isLoading: false };

    renderHook(() => useGitAuthStartupNotification());
    expect(toastFn).toHaveBeenCalledTimes(1);
  });
});
