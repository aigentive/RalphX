import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
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
  useGitRemoteUrl: vi.fn(),
  useGitAuthDiagnostics: vi.fn(),
  useGhAuthStatus: vi.fn(),
  useLoginGhWithBrowser: vi.fn(),
  useSwitchGitOriginToSsh: vi.fn(),
  useSetupGhGitAuth: vi.fn(),
  useResumeDeferredGitStartup: vi.fn(),
  useUpdateGithubPrEnabled: vi.fn(),
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
  useGitRemoteUrl,
  useGitAuthDiagnostics,
  useGhAuthStatus,
  useLoginGhWithBrowser,
  useSwitchGitOriginToSsh,
  useSetupGhGitAuth,
  useResumeDeferredGitStartup,
  useUpdateGithubPrEnabled,
} from "@/hooks/useGithubSettings";
import { useProjectStore } from "@/stores/projectStore";

const mockProject = {
  id: "proj-1",
  name: "Test Project",
  githubPrEnabled: false,
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

    vi.mocked(useGitRemoteUrl).mockReturnValue({
      data: "https://github.com/user/repo.git",
      isLoading: false,
    } as ReturnType<typeof useGitRemoteUrl>);

    vi.mocked(useGitAuthDiagnostics).mockReturnValue({
      data: {
        fetchUrl: "git@github.com:user/repo.git",
        pushUrl: "git@github.com:user/repo.git",
        fetchKind: "SSH",
        pushKind: "SSH",
        mixedAuthModes: false,
        canSwitchToSsh: false,
        suggestedSshUrl: null,
      },
      isLoading: false,
      isError: false,
      refetch: mockRefetchGitAuth,
    } as unknown as ReturnType<typeof useGitAuthDiagnostics>);

    vi.mocked(useGhAuthStatus).mockReturnValue({
      data: true,
      isLoading: false,
      isError: false,
      refetch: mockRefetchGhAuth,
    } as ReturnType<typeof useGhAuthStatus>);

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

  it("renders Repository section title", () => {
    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByText("Repository")).toBeInTheDocument();
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

  it("shows Authenticated when gh authed", () => {
    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByText("Authenticated")).toBeInTheDocument();
  });

  it("shows Not authenticated when gh not authed", () => {
    vi.mocked(useGhAuthStatus).mockReturnValue({
      data: false,
      isLoading: false,
      isError: false,
      refetch: mockRefetchGhAuth,
    } as ReturnType<typeof useGhAuthStatus>);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByText("Not authenticated")).toBeInTheDocument();
  });

  it("does not show a generic git repair warning for all-SSH remotes when PR mode is disabled", () => {
    vi.mocked(useGhAuthStatus).mockReturnValue({
      data: false,
      isLoading: false,
      isError: false,
      refetch: mockRefetchGhAuth,
    } as ReturnType<typeof useGhAuthStatus>);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.queryByTestId("git-auth-repair-panel")).not.toBeInTheDocument();
    expect(screen.queryByText(/GitHub CLI is not authenticated/i)).not.toBeInTheDocument();
  });

  it("shows an app-owned GitHub sign-in action when PR mode needs gh auth", () => {
    vi.mocked(useProjectStore).mockReturnValue({
      ...mockProject,
      githubPrEnabled: true,
    });
    vi.mocked(useGitRemoteUrl).mockReturnValue({
      data: "git@github.com:user/repo.git",
      isLoading: false,
    } as ReturnType<typeof useGitRemoteUrl>);
    vi.mocked(useGhAuthStatus).mockReturnValue({
      data: false,
      isLoading: false,
      isError: false,
      refetch: mockRefetchGhAuth,
    } as ReturnType<typeof useGhAuthStatus>);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    expect(screen.getByTestId("git-auth-repair-panel")).toBeInTheDocument();
    expect(screen.getByText("GitHub PR Access")).toBeInTheDocument();
    expect(screen.getByTestId("git-auth-login-gh")).toBeInTheDocument();
    expect(screen.queryByText(/Run gh auth login/i)).not.toBeInTheDocument();
  });

  it("disables PR mode toggle when remote is not GitHub", () => {
    vi.mocked(useGitRemoteUrl).mockReturnValue({
      data: "https://gitlab.com/user/repo.git",
      isLoading: false,
    } as ReturnType<typeof useGitRemoteUrl>);

    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const toggle = screen.getByTestId("github-pr-enabled");
    expect(toggle).toBeDisabled();
  });

  it("enables PR mode toggle when remote is GitHub", () => {
    render(<RepositorySettingsSection />, { wrapper: createWrapper() });

    const toggle = screen.getByTestId("github-pr-enabled");
    expect(toggle).not.toBeDisabled();
  });

  it("enables PR mode toggle for GitHub SSH remotes", () => {
    vi.mocked(useGitRemoteUrl).mockReturnValue({
      data: "git@github.com:user/repo.git",
      isLoading: false,
    } as ReturnType<typeof useGitRemoteUrl>);

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
        canSwitchToSsh: true,
        suggestedSshUrl: "git@github.com:user/repo.git",
      },
      isLoading: false,
      isError: false,
      refetch: mockRefetchGitAuth,
    } as unknown as ReturnType<typeof useGitAuthDiagnostics>);
    vi.mocked(useGhAuthStatus).mockReturnValue({
      data: false,
      isLoading: false,
      isError: false,
      refetch: mockRefetchGhAuth,
    } as ReturnType<typeof useGhAuthStatus>);

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
        canSwitchToSsh: false,
        suggestedSshUrl: null,
      },
      isError: false,
    });
    mockRefetchGhAuth.mockResolvedValue({
      data: true,
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

  it("disables PR mode toggle for URLs that only mention github.com in a query string", () => {
    vi.mocked(useGitRemoteUrl).mockReturnValue({
      data: "https://evil.example.com/redirect?target=https://github.com/user/repo.git",
      isLoading: false,
    } as ReturnType<typeof useGitRemoteUrl>);

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
});
