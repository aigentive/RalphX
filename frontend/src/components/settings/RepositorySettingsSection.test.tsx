import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RepositorySettingsSection } from "./RepositorySettingsSection";

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

vi.mock("@/hooks/useGithubSettings", () => ({
  useGitAuthDiagnostics: vi.fn(),
  useLoginGhWithBrowser: vi.fn(),
  useSwitchGitOriginToSsh: vi.fn(),
  useSetupGhGitAuth: vi.fn(),
  useResumeDeferredGitStartup: vi.fn(),
  useUpdateGithubPrEnabled: vi.fn(),
}));

vi.mock("@/hooks/useGitHubConnectionStatus", () => ({
  useGitHubConnectionStatus: vi.fn(),
}));

vi.mock("@/hooks/useAgentGate", () => ({
  useAgentGate: () => ({ status: "enabled", gated: false, reason: null }),
}));

// `GitAuthRepairPanel` reads the bus through `useEventBus()` rather than raw `listen`, so
// rendering it needs a bus in context. Mocked like the other hooks here — mounting the real
// EventProvider would pull in every global listener this unit test has no business running.
vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => {
    const unsubscribe = Object.assign(() => {}, {
      ready: Promise.resolve(),
    });
    return {
      subscribe: () => unsubscribe,
      emit: () => {},
    };
  },
}));

const mockUpdateProject = vi.fn();
vi.mock("@/stores/projectStore", () => ({
  useProjectStore: vi.fn(),
  selectActiveProject: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({
  api: {
    projects: {
      update: vi.fn(),
    },
  },
  getGitDefaultBranch: vi.fn(),
}));

import {
  useGitAuthDiagnostics,
  useLoginGhWithBrowser,
  useSwitchGitOriginToSsh,
  useSetupGhGitAuth,
  useResumeDeferredGitStartup,
  useUpdateGithubPrEnabled,
} from "@/hooks/useGithubSettings";
import { useGitHubConnectionStatus } from "@/hooks/useGitHubConnectionStatus";
import { useProjectStore } from "@/stores/projectStore";
import { api, getGitDefaultBranch } from "@/lib/tauri";
import type { Project } from "@/types/project";

const mockProject = {
  id: "proj-1",
  name: "Test Project",
  githubPrEnabled: false,
  repositoryCapability: {
    kind: "github" as const,
    fetchUrl: "https://github.com/user/repo.git",
    pushUrl: "https://github.com/user/repo.git",
  },
  workingDirectory: "/home/user/project",
  baseBranch: "main",
  useFeatureBranches: false,
  mergeValidationMode: "block" as const,
  worktreeParentDirectory: null,
  createdAt: "2024-01-01T00:00:00Z",
  updatedAt: "2024-01-01T00:00:00Z",
};

const mockMutateAsync = vi.fn();
const mockSwitchToSsh = vi.fn();
const mockSetupGhGitAuth = vi.fn();
const mockLoginGhWithBrowser = vi.fn();
const mockResumeDeferredGitStartup = vi.fn();
const mockRefetchGitAuth = vi.fn();
const mockRefetchGhAuth = vi.fn();

function mockProjectCapability(
  changes: Pick<Project, "githubPrEnabled" | "repositoryCapability">,
) {
  vi.mocked(useProjectStore).mockImplementation(((selector: unknown) => {
    const state = { updateProject: mockUpdateProject };
    if (typeof selector === "function") {
      const result = (selector as (s: unknown) => unknown)(state);
      return result === undefined ? { ...mockProject, ...changes } : result;
    }
    return mockProject;
  }) as never);
}

function ghStatus(state: "authenticated" | "unauthenticated" | "credential_rejected" | "provider_unavailable" | "cli_unavailable" | "probe_failed") {
  return {
    state,
    diagnostic:
      state === "authenticated"
        ? null
        : state === "provider_unavailable"
          ? "http5xx"
          : state === "credential_rejected"
            ? "credentials_rejected"
            : state === "cli_unavailable"
              ? "cli_launch"
              : "missing_credentials",
    ghInstalled: state !== "cli_unavailable",
    authenticated: state === "authenticated",
    host: state === "cli_unavailable" ? null : "github.com",
    account: state === "authenticated" ? "octocat" : null,
  };
}

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

describe("RepositorySettingsSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockMutateAsync.mockReset();
    mockSwitchToSsh.mockReset();
    mockSetupGhGitAuth.mockReset();
    mockLoginGhWithBrowser.mockReset();
    mockResumeDeferredGitStartup.mockReset();
    mockRefetchGitAuth.mockReset();
    mockRefetchGhAuth.mockReset();

    mockUpdateProject.mockReset();
    vi.mocked(useProjectStore).mockImplementation(((selector: unknown) => {
      const state = { updateProject: mockUpdateProject };
      if (typeof selector === "function") {
        // selectActiveProject is a vi.fn() returning undefined; treat as project-getter
        const result = (selector as (s: unknown) => unknown)(state);
        return result === undefined ? mockProject : result;
      }
      return mockProject;
    }) as never);

    vi.mocked(useGitAuthDiagnostics).mockReturnValue({
      data: {
        fetchUrl: "git@github.com:user/repo.git",
        pushUrl: "git@github.com:user/repo.git",
        fetchKind: "SSH",
        pushKind: "SSH",
        mixedAuthModes: false,
        githubHttpsCredentialHelperConfigured: false,
        canSwitchToSsh: false,
        suggestedSshUrl: null,
      },
      isLoading: false,
      isError: false,
      refetch: mockRefetchGitAuth,
    } as unknown as ReturnType<typeof useGitAuthDiagnostics>);

    vi.mocked(useGitHubConnectionStatus).mockReturnValue({
      data: ghStatus("authenticated"),
      isLoading: false,
      isError: false,
      refetch: mockRefetchGhAuth,
    } as ReturnType<typeof useGitHubConnectionStatus>);

    vi.mocked(useSwitchGitOriginToSsh).mockReturnValue({
      mutateAsync: mockSwitchToSsh,
      isPending: false,
    } as unknown as ReturnType<typeof useSwitchGitOriginToSsh>);

    vi.mocked(useSetupGhGitAuth).mockReturnValue({
      mutateAsync: mockSetupGhGitAuth,
      isPending: false,
    } as unknown as ReturnType<typeof useSetupGhGitAuth>);

    vi.mocked(useLoginGhWithBrowser).mockReturnValue({
      mutateAsync: mockLoginGhWithBrowser,
      isPending: false,
    } as unknown as ReturnType<typeof useLoginGhWithBrowser>);

    vi.mocked(useResumeDeferredGitStartup).mockReturnValue({
      mutateAsync: mockResumeDeferredGitStartup,
      isPending: false,
    } as unknown as ReturnType<typeof useResumeDeferredGitStartup>);

    vi.mocked(useUpdateGithubPrEnabled).mockReturnValue({
      mutateAsync: mockMutateAsync,
      isPending: false,
    } as unknown as ReturnType<typeof useUpdateGithubPrEnabled>);
  });

  it("renders null when no project selected", () => {
    vi.mocked(useProjectStore).mockReturnValue(null);

    const { container } = render(<RepositorySettingsSection />, {
      wrapper: createWrapper(),
    });

    expect(container.firstChild).toBeNull();
  });

  it("renders Branching, Merge Behavior, and Diagnostics subsections", () => {
    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByText("Branching")).toBeInTheDocument();
    expect(screen.getByText("Merge Behavior")).toBeInTheDocument();
    expect(screen.getByText("Diagnostics")).toBeInTheDocument();
  });

  it("shows remote URL in Diagnostics", () => {
    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(
      screen.getByText("https://github.com/user/repo.git")
    ).toBeInTheDocument();
  });

  it("uses live local-only capability rather than the remote URL query to disable future PR mode", () => {
    mockProjectCapability({
      githubPrEnabled: false,
      repositoryCapability: { kind: "localOnly" },
    });

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByText("Local workflows available")).toBeInTheDocument();
    expect(screen.getByTestId("github-pr-enabled")).toBeDisabled();
  });

  it("shows Authenticated when gh authed", () => {
    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByText("Authenticated")).toBeInTheDocument();
  });

  it("shows Not authenticated when gh not authed", () => {
    vi.mocked(useGitHubConnectionStatus).mockReturnValue({
      data: ghStatus("unauthenticated"),
      isLoading: false,
      isError: false,
      refetch: mockRefetchGhAuth,
    } as ReturnType<typeof useGitHubConnectionStatus>);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByText("Not authenticated")).toBeInTheDocument();
  });

  it("shows git auth panel for all-SSH remotes in settings (showWhenHealthy) without a generic repair warning", () => {
    vi.mocked(useGitHubConnectionStatus).mockReturnValue({
      data: ghStatus("unauthenticated"),
      isLoading: false,
      isError: false,
      refetch: mockRefetchGhAuth,
    } as ReturnType<typeof useGitHubConnectionStatus>);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByTestId("git-auth-repair-panel")).toBeInTheDocument();
    expect(screen.queryByText(/GitHub CLI is not authenticated/i)).not.toBeInTheDocument();
  });

  it("shows an app-owned GitHub sign-in action when PR mode needs gh auth", () => {
    vi.mocked(useProjectStore).mockReturnValue({
      ...mockProject,
      githubPrEnabled: true,
    });
    vi.mocked(useGitHubConnectionStatus).mockReturnValue({
      data: ghStatus("unauthenticated"),
      isLoading: false,
      isError: false,
      refetch: mockRefetchGhAuth,
    } as ReturnType<typeof useGitHubConnectionStatus>);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByTestId("git-auth-repair-panel")).toBeInTheDocument();
    expect(screen.getByText("GitHub PR Access")).toBeInTheDocument();
    expect(screen.getByTestId("git-auth-login-gh")).toBeInTheDocument();
    expect(screen.queryByText(/Run gh auth login/i)).not.toBeInTheDocument();
  });

  it("disables PR mode toggle when the repository capability is not GitHub", () => {
    mockProjectCapability({
      githubPrEnabled: false,
      repositoryCapability: {
        kind: "otherRemote",
        fetchUrl: "https://gitlab.com/user/repo.git",
        pushUrl: "https://gitlab.com/user/repo.git",
      },
    });

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const toggle = screen.getByTestId("github-pr-enabled");
    expect(toggle).toBeDisabled();
  });

  it("enables PR mode toggle when remote is GitHub", () => {
    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const toggle = screen.getByTestId("github-pr-enabled");
    expect(toggle).not.toBeDisabled();
  });

  it("enables PR mode toggle for GitHub SSH capabilities", () => {
    mockProjectCapability({
      githubPrEnabled: false,
      repositoryCapability: {
        kind: "github",
        fetchUrl: "git@github.com:user/repo.git",
        pushUrl: "git@github.com:user/repo.git",
      },
    });

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const toggle = screen.getByTestId("github-pr-enabled");
    expect(toggle).not.toBeDisabled();
  });

  it("surfaces git auth repair actions in diagnostics", () => {
    vi.mocked(useGitAuthDiagnostics).mockReturnValue({
      data: {
        fetchUrl: "https://github.com/user/repo.git",
        pushUrl: "git@github.com:user/repo.git",
        fetchKind: "HTTPS",
        pushKind: "SSH",
        mixedAuthModes: true,
        githubHttpsCredentialHelperConfigured: false,
        canSwitchToSsh: true,
        suggestedSshUrl: "git@github.com:user/repo.git",
      },
      isLoading: false,
      isError: false,
      refetch: mockRefetchGitAuth,
    } as unknown as ReturnType<typeof useGitAuthDiagnostics>);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByTestId("git-auth-repair-panel")).toBeInTheDocument();
    expect(screen.getByText(/Fetch and push use different auth modes/i)).toBeInTheDocument();
    expect(screen.getByTestId("git-auth-switch-ssh")).toBeInTheDocument();
    expect(screen.getByTestId("git-auth-setup-gh")).toBeInTheDocument();
  });

  it("shows an HTTPS setup path when GitHub CLI is not authenticated", () => {
    vi.mocked(useGitAuthDiagnostics).mockReturnValue({
      data: {
        fetchUrl: "https://github.com/user/repo.git",
        pushUrl: "https://github.com/user/repo.git",
        fetchKind: "HTTPS",
        pushKind: "HTTPS",
        mixedAuthModes: false,
        githubHttpsCredentialHelperConfigured: false,
        canSwitchToSsh: true,
        suggestedSshUrl: "git@github.com:user/repo.git",
      },
      isLoading: false,
      isError: false,
      refetch: mockRefetchGitAuth,
    } as unknown as ReturnType<typeof useGitAuthDiagnostics>);
    vi.mocked(useGitHubConnectionStatus).mockReturnValue({
      data: ghStatus("unauthenticated"),
      isLoading: false,
      isError: false,
      refetch: mockRefetchGhAuth,
    } as ReturnType<typeof useGitHubConnectionStatus>);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByTestId("git-auth-switch-ssh")).toBeInTheDocument();
    expect(screen.getByTestId("git-auth-copy-gh-login")).toBeInTheDocument();
    expect(screen.queryByTestId("git-auth-setup-gh")).not.toBeInTheDocument();
  });

  it("rechecks and resumes deferred startup recovery once auth is healthy", async () => {
    const user = userEvent.setup();
    mockRefetchGitAuth.mockResolvedValue({
      data: {
        fetchUrl: "git@github.com:user/repo.git",
        pushUrl: "git@github.com:user/repo.git",
        fetchKind: "SSH",
        pushKind: "SSH",
        mixedAuthModes: false,
        githubHttpsCredentialHelperConfigured: false,
        canSwitchToSsh: false,
        suggestedSshUrl: null,
      },
      isError: false,
    });
    mockRefetchGhAuth.mockResolvedValue({
      data: ghStatus("authenticated"),
      isError: false,
    });
    mockResumeDeferredGitStartup.mockResolvedValue(true);
    vi.mocked(useGitAuthDiagnostics).mockReturnValue({
      data: {
        fetchUrl: "https://github.com/user/repo.git",
        pushUrl: "git@github.com:user/repo.git",
        fetchKind: "HTTPS",
        pushKind: "SSH",
        mixedAuthModes: true,
        githubHttpsCredentialHelperConfigured: false,
        canSwitchToSsh: true,
        suggestedSshUrl: "git@github.com:user/repo.git",
      },
      isLoading: false,
      isError: false,
      refetch: mockRefetchGitAuth,
    } as unknown as ReturnType<typeof useGitAuthDiagnostics>);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    await user.click(screen.getByTestId("git-auth-recheck"));

    await waitFor(() => {
      expect(mockResumeDeferredGitStartup).toHaveBeenCalledTimes(1);
    });
  });

  it("disables PR mode toggle for non-GitHub capability even with a GitHub-looking URL", () => {
    mockProjectCapability({
      githubPrEnabled: false,
      repositoryCapability: {
        kind: "otherRemote",
        fetchUrl: "https://evil.example.com/redirect?target=https://github.com/user/repo.git",
        pushUrl: "https://evil.example.com/redirect?target=https://github.com/user/repo.git",
      },
    });

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const toggle = screen.getByTestId("github-pr-enabled");
    expect(toggle).toBeDisabled();
  });

  it("calls updatePrEnabled.mutateAsync on PR toggle", async () => {
    const user = userEvent.setup();
    mockMutateAsync.mockResolvedValue(undefined);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const toggle = screen.getByTestId("github-pr-enabled");
    await user.click(toggle);

    await waitFor(() => {
      expect(mockMutateAsync).toHaveBeenCalledWith({
        projectId: "proj-1",
        enabled: true,
      });
    });
  });

  it("shows base-branch and worktree-location inputs", () => {
    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByTestId("base-branch")).toBeInTheDocument();
    expect(screen.getByTestId("worktree-location")).toBeInTheDocument();
  });

  it("shows merge validation select", () => {
    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByTestId("merge-validation-mode")).toBeInTheDocument();
  });

  it("shows 'Not configured' when a local-only repository has no remote", () => {
    mockProjectCapability({
      githubPrEnabled: false,
      repositoryCapability: { kind: "localOnly" },
    });

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByText("Not configured")).toBeInTheDocument();
  });

  it("disables PR toggle when capability inspection fails", () => {
    mockProjectCapability({
      githubPrEnabled: false,
      repositoryCapability: {
        kind: "inspectionFailed",
        message: "Could not inspect origin",
      },
    });

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const toggle = screen.getByTestId("github-pr-enabled");
    expect(toggle).toBeDisabled();
  });

  it("renders an honest not-inspected capability without a fabricated URL", () => {
    mockProjectCapability({
      githubPrEnabled: false,
      repositoryCapability: { kind: "notInspected" },
    });

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByTestId("github-pr-enabled")).toBeDisabled();
    expect(screen.getByText("The host has not inspected this repository yet.")).toBeInTheDocument();
    expect(screen.getByText("Not inspected yet")).toBeInTheDocument();
    expect(screen.getByText("—")).toBeInTheDocument();
  });

  it("commits base branch on blur and shows success toast", async () => {
    vi.mocked(api.projects.update).mockResolvedValue(undefined as never);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const input = screen.getByTestId("base-branch") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "develop" } });
    fireEvent.blur(input);

    await waitFor(() => {
      expect(api.projects.update).toHaveBeenCalledWith("proj-1", {
        baseBranch: "develop",
      });
    });
    expect(mockUpdateProject).toHaveBeenCalledWith("proj-1", {
      baseBranch: "develop",
    });
    expect(toastSuccess).toHaveBeenCalledWith("Base branch updated");
  });

  it("does not commit base branch when value unchanged on blur", async () => {
    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const input = screen.getByTestId("base-branch") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "main" } });
    fireEvent.blur(input);

    await new Promise((r) => setTimeout(r, 0));
    expect(api.projects.update).not.toHaveBeenCalled();
  });

  it("shows error toast when base branch update fails", async () => {
    vi.mocked(api.projects.update).mockRejectedValueOnce(new Error("nope"));

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const input = screen.getByTestId("base-branch") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "feature" } });
    fireEvent.blur(input);

    await waitFor(() => {
      expect(toastError).toHaveBeenCalledWith("nope");
    });
  });

  it("falls back to default toast message on non-Error throw", async () => {
    vi.mocked(api.projects.update).mockRejectedValueOnce("oops");

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const input = screen.getByTestId("base-branch") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "feature" } });
    fireEvent.blur(input);

    await waitFor(() => {
      expect(toastError).toHaveBeenCalledWith("Failed to update base branch");
    });
  });

  it("detects default branch and updates project", async () => {
    vi.mocked(getGitDefaultBranch).mockResolvedValueOnce("trunk");
    vi.mocked(api.projects.update).mockResolvedValue(undefined as never);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    fireEvent.click(screen.getByTitle("Detect"));

    await waitFor(() => {
      expect(getGitDefaultBranch).toHaveBeenCalledWith("/home/user/project");
    });
    await waitFor(() => {
      expect(api.projects.update).toHaveBeenCalledWith("proj-1", {
        baseBranch: "trunk",
      });
    });
    expect(toastSuccess).toHaveBeenCalledWith("Detected default branch: trunk");
  });

  it("shows error toast when no working directory configured for detect", async () => {
    vi.mocked(useProjectStore).mockImplementation(((selector: unknown) => {
      const proj = { ...mockProject, workingDirectory: null };
      const state = { updateProject: mockUpdateProject };
      if (typeof selector === "function") {
        const res = (selector as (s: unknown) => unknown)(state);
        return res === undefined ? proj : res;
      }
      return proj;
    }) as never);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    fireEvent.click(screen.getByTitle("Detect"));

    await waitFor(() => {
      expect(toastError).toHaveBeenCalledWith(
        "No working directory set for this project"
      );
    });
    expect(getGitDefaultBranch).not.toHaveBeenCalled();
  });

  it("shows error toast when default-branch detection fails", async () => {
    vi.mocked(getGitDefaultBranch).mockRejectedValueOnce(new Error("git error"));

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    fireEvent.click(screen.getByTitle("Detect"));

    await waitFor(() => {
      expect(toastError).toHaveBeenCalledWith("git error");
    });
  });

  it("commits worktree directory change on blur", async () => {
    vi.mocked(api.projects.update).mockResolvedValue(undefined as never);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const input = screen.getByTestId("worktree-location") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "/tmp/wt" } });
    fireEvent.blur(input);

    await waitFor(() => {
      expect(api.projects.update).toHaveBeenCalledWith("proj-1", {
        worktreeParentDirectory: "/tmp/wt",
      });
    });
    expect(toastSuccess).toHaveBeenCalledWith("Worktree location updated");
  });

  it("does not commit worktree change when value matches default", async () => {
    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const input = screen.getByTestId("worktree-location") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "~/ralphx-worktrees" } });
    fireEvent.blur(input);

    await new Promise((r) => setTimeout(r, 0));
    expect(api.projects.update).not.toHaveBeenCalled();
  });

  it("shows error toast when worktree update fails", async () => {
    vi.mocked(api.projects.update).mockRejectedValueOnce(new Error("io fail"));

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const input = screen.getByTestId("worktree-location") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "/tmp/wt" } });
    fireEvent.blur(input);

    await waitFor(() => {
      expect(toastError).toHaveBeenCalledWith("io fail");
    });
  });

  it("shows error toast when PR toggle mutation fails", async () => {
    mockMutateAsync.mockRejectedValueOnce(new Error("toggle fail"));

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    fireEvent.click(screen.getByTestId("github-pr-enabled"));

    await waitFor(() => {
      expect(toastError).toHaveBeenCalledWith("toggle fail");
    });
  });

  it("shows Saving indicator while PR mutation pending", () => {
    vi.mocked(useUpdateGithubPrEnabled).mockReturnValue({
      mutateAsync: mockMutateAsync,
      isPending: true,
    } as unknown as ReturnType<typeof useUpdateGithubPrEnabled>);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByText("Saving...")).toBeInTheDocument();
  });
});
