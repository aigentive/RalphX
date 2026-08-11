/**
 * Tests for GitAuthRepairPanel — focused on the restored Sign-in button wiring,
 * the login-prompt event flow, and the recheck/resume happy path.
 */

import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { mocks } = vi.hoisted(() => ({
  mocks: {
    diagnostics: {
      data: undefined as unknown,
      isLoading: false,
      isError: false,
      error: null as unknown,
      refetch: vi.fn().mockResolvedValue({ data: undefined, isError: false }),
    },
    ghAuth: {
      data: undefined as unknown,
      isLoading: false,
      refetch: vi.fn().mockResolvedValue({ data: undefined }),
      isError: false,
    },
    loginGh: { mutateAsync: vi.fn().mockResolvedValue(undefined), isPending: false },
    setupGh: { mutateAsync: vi.fn().mockResolvedValue(undefined), isPending: false },
    switchSsh: { mutateAsync: vi.fn().mockResolvedValue(undefined), isPending: false },
    resumeDeferred: {
      mutateAsync: vi.fn().mockResolvedValue(false),
      isPending: false,
    },
    listen: vi.fn(),
  },
}));

vi.mock("@/hooks/useGithubSettings", () => ({
  useGitAuthDiagnostics: () => mocks.diagnostics,
  useLoginGhWithBrowser: () => mocks.loginGh,
  useSetupGhGitAuth: () => mocks.setupGh,
  useSwitchGitOriginToSsh: () => mocks.switchSsh,
  useResumeDeferredGitStartup: () => mocks.resumeDeferred,
}));

vi.mock("@/hooks/useGitHubConnectionStatus", () => ({
  useGitHubConnectionStatus: () => mocks.ghAuth,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => mocks.listen(...args),
}));

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), {
    success: vi.fn(),
    error: vi.fn(),
    dismiss: vi.fn(),
  }),
}));

vi.mock("./GhAuthLoginPrompt", () => ({
  GhAuthLoginPrompt: ({ prompt }: { prompt: { code?: string | null; url?: string | null } }) => (
    <div data-testid="gh-auth-login-prompt">
      <span>{prompt.code ?? ""}</span>
      <span>{prompt.url ?? ""}</span>
    </div>
  ),
}));

vi.mock("./GitAuthTerminalSetupButton", () => ({
  GitAuthTerminalSetupButton: ({ onCopy }: { onCopy: () => void }) => (
    <button type="button" data-testid="git-auth-terminal-copy" onClick={onCopy}>
      Use Terminal
    </button>
  ),
}));

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: () => ({
    confirm: vi.fn().mockResolvedValue(true),
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  }),
}));

import { GitAuthRepairPanel } from "./GitAuthRepairPanel";

beforeEach(() => {
  mocks.listen.mockReset();
  mocks.loginGh.mutateAsync.mockReset();
  mocks.loginGh.mutateAsync.mockResolvedValue(undefined);
  mocks.resumeDeferred.mutateAsync.mockReset();
  mocks.resumeDeferred.mutateAsync.mockResolvedValue(false);
  mocks.diagnostics.data = {
    fetchUrl: "https://github.com/owner/repo.git",
    pushUrl: "https://github.com/owner/repo.git",
    fetchKind: "HTTPS",
    pushKind: "HTTPS",
    mixedAuthModes: false,
    githubHttpsCredentialHelperConfigured: false,
    canSwitchToSsh: false,
    suggestedSshUrl: null,
  };
  mocks.diagnostics.isError = false;
  mocks.diagnostics.isLoading = false;
  mocks.diagnostics.refetch = vi
    .fn()
    .mockResolvedValue({ data: mocks.diagnostics.data, isError: false });
  mocks.ghAuth.data = {
    state: "unauthenticated",
    diagnostic: "missing_credentials",
    ghInstalled: true,
    authenticated: false,
    host: "github.com",
    account: null,
  };
  mocks.ghAuth.isLoading = false;
  mocks.ghAuth.isError = false;
  mocks.ghAuth.refetch = vi.fn().mockResolvedValue({ data: mocks.ghAuth.data });
});

describe("GitAuthRepairPanel — Sign in", () => {
  it("clicking Sign in invokes the login mutation and opens the listen pipe", async () => {
    const user = userEvent.setup();
    let listenCallback: ((event: { payload: unknown }) => void) | null = null;
    mocks.listen.mockImplementation((_event, cb) => {
      listenCallback = cb as typeof listenCallback;
      return Promise.resolve(() => undefined);
    });

    render(<GitAuthRepairPanel projectId="proj-1" />);
    await user.click(screen.getByTestId("git-auth-login-gh"));

    expect(mocks.listen).toHaveBeenCalledWith("gh-auth:login_prompt", expect.any(Function));
    expect(mocks.loginGh.mutateAsync).toHaveBeenCalled();

    // Simulate the device-code event payload arriving — it should render GhAuthLoginPrompt.
    listenCallback?.({ payload: { code: "ABCD-1234", url: "https://github.com/login/device" } });
    await waitFor(() =>
      expect(screen.queryByTestId("gh-auth-login-prompt")).toBeInTheDocument(),
    );
  });

  it("renders nothing when projectId is null", () => {
    const { container } = render(<GitAuthRepairPanel projectId={null} />);
    expect(container.innerHTML).toBe("");
  });

  it("hides the Sign in button when GitHub CLI is already signed in (no HTTPS issue)", () => {
    mocks.ghAuth.data = {
      state: "authenticated",
      diagnostic: null,
      ghInstalled: true,
      authenticated: true,
      host: "github.com",
      account: "octocat",
    };
    mocks.diagnostics.data = {
      ...(mocks.diagnostics.data as object),
      fetchKind: "SSH",
      pushKind: "SSH",
      fetchUrl: "git@github.com:owner/repo.git",
      pushUrl: "git@github.com:owner/repo.git",
    };
    render(<GitAuthRepairPanel projectId="proj-1" />);
    expect(screen.queryByTestId("git-auth-login-gh")).toBeNull();
  });

  it("renders the Use Terminal fallback button alongside Sign in", () => {
    render(<GitAuthRepairPanel projectId="proj-1" />);
    expect(screen.getByTestId("git-auth-login-gh")).toBeInTheDocument();
    expect(screen.getByTestId("git-auth-terminal-copy")).toBeInTheDocument();
  });

  it("Use SSH triggers the switch-to-ssh mutation", async () => {
    const user = userEvent.setup();
    mocks.diagnostics.data = {
      fetchUrl: "https://github.com/owner/repo.git",
      pushUrl: "https://github.com/owner/repo.git",
      fetchKind: "HTTPS",
      pushKind: "HTTPS",
      mixedAuthModes: false,
      githubHttpsCredentialHelperConfigured: false,
      canSwitchToSsh: true,
      suggestedSshUrl: "git@github.com:owner/repo.git",
    };
    mocks.diagnostics.refetch = vi
      .fn()
      .mockResolvedValue({ data: mocks.diagnostics.data, isError: false });

    render(<GitAuthRepairPanel projectId="proj-1" />);
    await user.click(screen.getByTestId("git-auth-switch-ssh"));
    expect(mocks.switchSsh.mutateAsync).toHaveBeenCalledWith({ projectId: "proj-1" });
  });

  it("Setup HTTPS triggers the setup-gh mutation when gh is signed in on HTTPS remote", async () => {
    const user = userEvent.setup();
    mocks.ghAuth.data = {
      state: "authenticated",
      diagnostic: null,
      ghInstalled: true,
      authenticated: true,
      host: "github.com",
      account: "octocat",
    };
    render(<GitAuthRepairPanel projectId="proj-1" />);
    await user.click(screen.getByTestId("git-auth-setup-gh"));
    expect(mocks.setupGh.mutateAsync).toHaveBeenCalled();
  });

  it("treats GitHub HTTPS as healthy when gh auth and credential helper are configured", () => {
    mocks.ghAuth.data = {
      state: "authenticated",
      diagnostic: null,
      ghInstalled: true,
      authenticated: true,
      host: "github.com",
      account: "octocat",
    };
    mocks.diagnostics.data = {
      fetchUrl: "https://github.com/owner/repo.git",
      pushUrl: "https://github.com/owner/repo.git",
      fetchKind: "HTTPS",
      pushKind: "HTTPS",
      mixedAuthModes: false,
      githubHttpsCredentialHelperConfigured: true,
      canSwitchToSsh: true,
      suggestedSshUrl: "git@github.com:owner/repo.git",
    };

    render(<GitAuthRepairPanel projectId="proj-1" />);

    expect(screen.queryByText(/HTTPS remotes need/i)).not.toBeInTheDocument();
    expect(screen.queryByTestId("git-auth-setup-gh")).not.toBeInTheDocument();
    expect(screen.getByText("Git remote auth and GitHub CLI status look ready.")).toBeInTheDocument();
    expect(screen.getByTestId("git-auth-switch-ssh")).toBeInTheDocument();
  });

  it("hides on publish surface when auth is healthy even if canSwitchToSsh is available", () => {
    mocks.ghAuth.data = {
      state: "authenticated",
      diagnostic: null,
      ghInstalled: true,
      authenticated: true,
      host: "github.com",
      account: "octocat",
    };
    mocks.diagnostics.data = {
      fetchUrl: "https://github.com/owner/repo.git",
      pushUrl: "https://github.com/owner/repo.git",
      fetchKind: "HTTPS",
      pushKind: "HTTPS",
      mixedAuthModes: false,
      githubHttpsCredentialHelperConfigured: true,
      canSwitchToSsh: true,
      suggestedSshUrl: "git@github.com:owner/repo.git",
    };

    const { container } = render(
      <GitAuthRepairPanel projectId="proj-1" surface="publish" requiresGhAuth />,
    );
    expect(container.innerHTML).toBe("");
  });

  it("shows on publish surface when there is a visible issue", () => {
    mocks.ghAuth.data = {
      state: "unauthenticated",
      diagnostic: "missing_credentials",
      ghInstalled: true,
      authenticated: false,
      host: "github.com",
      account: null,
    };
    mocks.diagnostics.data = {
      fetchUrl: "https://github.com/owner/repo.git",
      pushUrl: "https://github.com/owner/repo.git",
      fetchKind: "HTTPS",
      pushKind: "HTTPS",
      mixedAuthModes: false,
      githubHttpsCredentialHelperConfigured: false,
      canSwitchToSsh: true,
      suggestedSshUrl: "git@github.com:owner/repo.git",
    };

    render(
      <GitAuthRepairPanel projectId="proj-1" surface="publish" requiresGhAuth />,
    );
    expect(screen.getByTestId("git-auth-repair-panel")).toBeInTheDocument();
  });

  it("shows on settings surface with showWhenHealthy even when auth is healthy", () => {
    mocks.ghAuth.data = {
      state: "authenticated",
      diagnostic: null,
      ghInstalled: true,
      authenticated: true,
      host: "github.com",
      account: "octocat",
    };
    mocks.diagnostics.data = {
      fetchUrl: "git@github.com:owner/repo.git",
      pushUrl: "git@github.com:owner/repo.git",
      fetchKind: "SSH",
      pushKind: "SSH",
      mixedAuthModes: false,
      githubHttpsCredentialHelperConfigured: false,
      canSwitchToSsh: false,
      suggestedSshUrl: null,
    };

    render(
      <GitAuthRepairPanel projectId="proj-1" showWhenHealthy />,
    );
    expect(screen.getByTestId("git-auth-repair-panel")).toBeInTheDocument();
    expect(screen.getByText("Git remote auth and GitHub CLI status look ready.")).toBeInTheDocument();
  });

  it("Recheck refetches diagnostics and gh auth status", async () => {
    const user = userEvent.setup();
    render(<GitAuthRepairPanel projectId="proj-1" />);
    await user.click(screen.getByTestId("git-auth-recheck"));
    expect(mocks.diagnostics.refetch).toHaveBeenCalled();
    expect(mocks.ghAuth.refetch).toHaveBeenCalled();
  });

  it("does not show Sign in for transient provider failures", () => {
    mocks.ghAuth.data = {
      state: "provider_unavailable",
      diagnostic: "http5xx",
      ghInstalled: true,
      authenticated: false,
      host: "github.com",
      account: null,
    };
    mocks.diagnostics.data = {
      fetchUrl: "git@github.com:owner/repo.git",
      pushUrl: "git@github.com:owner/repo.git",
      fetchKind: "SSH",
      pushKind: "SSH",
      mixedAuthModes: false,
      githubHttpsCredentialHelperConfigured: false,
      canSwitchToSsh: false,
      suggestedSshUrl: null,
    };

    render(<GitAuthRepairPanel projectId="proj-1" requiresGhAuth />);

    expect(screen.getByText(/temporarily unavailable/i)).toBeInTheDocument();
    expect(screen.queryByTestId("git-auth-login-gh")).not.toBeInTheDocument();
    expect(screen.getByTestId("git-auth-recheck")).toBeInTheDocument();
  });

  it("PR-only access mode shows the title 'GitHub PR Access'", () => {
    mocks.diagnostics.data = {
      fetchUrl: "git@github.com:owner/repo.git",
      pushUrl: "git@github.com:owner/repo.git",
      fetchKind: "SSH",
      pushKind: "SSH",
      mixedAuthModes: false,
      githubHttpsCredentialHelperConfigured: false,
      canSwitchToSsh: false,
      suggestedSshUrl: null,
    };
    mocks.ghAuth.data = {
      state: "credential_rejected",
      diagnostic: "credentials_rejected",
      ghInstalled: true,
      authenticated: false,
      host: "github.com",
      account: null,
    };

    render(<GitAuthRepairPanel projectId="proj-1" requiresGhAuth />);
    expect(screen.getByText("GitHub PR Access")).toBeInTheDocument();
  });

  it("Sign in mutation failure surfaces an error toast", async () => {
    const user = userEvent.setup();
    const sonner = await import("sonner");
    mocks.listen.mockResolvedValue(() => undefined);
    mocks.loginGh.mutateAsync.mockReset();
    mocks.loginGh.mutateAsync.mockRejectedValue(new Error("nope"));

    render(<GitAuthRepairPanel projectId="proj-1" />);
    await user.click(screen.getByTestId("git-auth-login-gh"));
    await waitFor(() => expect(sonner.toast.error).toHaveBeenCalled());
  });

  it("Recheck triggers resume mutation and success toast when diagnostics are healthy", async () => {
    const user = userEvent.setup();
    const sonner = await import("sonner");
    // Healthy state — SSH remote, gh authenticated, no blocking issue
    const healthyDiagnostics = {
      fetchUrl: "git@github.com:owner/repo.git",
      pushUrl: "git@github.com:owner/repo.git",
      fetchKind: "SSH",
      pushKind: "SSH",
      mixedAuthModes: false,
      canSwitchToSsh: false,
      suggestedSshUrl: null,
    };
    mocks.diagnostics.data = healthyDiagnostics;
    mocks.diagnostics.refetch = vi
      .fn()
      .mockResolvedValue({ data: healthyDiagnostics, isError: false });
    const healthyStatus = {
      state: "authenticated",
      diagnostic: null,
      ghInstalled: true,
      authenticated: true,
      host: "github.com",
      account: "octocat",
    };
    mocks.ghAuth.data = healthyStatus;
    mocks.ghAuth.refetch = vi.fn().mockResolvedValue({ data: healthyStatus });
    mocks.resumeDeferred.mutateAsync.mockReset();
    mocks.resumeDeferred.mutateAsync.mockResolvedValue(true);

    render(<GitAuthRepairPanel projectId="proj-1" showWhenHealthy />);
    await user.click(screen.getByTestId("git-auth-recheck"));

    await waitFor(() => expect(mocks.resumeDeferred.mutateAsync).toHaveBeenCalled());
    await waitFor(() =>
      expect(sonner.toast.success).toHaveBeenCalledWith(
        "Deferred startup recovery resumed",
      ),
    );
  });

  it("Use Terminal copies the gh login command to the clipboard and shows success toast", async () => {
    const user = userEvent.setup();
    const sonner = await import("sonner");
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    render(<GitAuthRepairPanel projectId="proj-1" />);
    await user.click(screen.getByTestId("git-auth-terminal-copy"));

    await waitFor(() => expect(writeText).toHaveBeenCalled());
    expect(writeText.mock.calls[0]?.[0]).toContain("gh auth login");
    await waitFor(() =>
      expect(sonner.toast.success).toHaveBeenCalledWith(
        "Terminal sign-in command copied",
      ),
    );
  });

  it("Use Terminal surfaces an error toast when clipboard write fails", async () => {
    const user = userEvent.setup();
    const sonner = await import("sonner");
    const writeText = vi.fn().mockRejectedValue(new Error("denied"));
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    render(<GitAuthRepairPanel projectId="proj-1" />);
    await user.click(screen.getByTestId("git-auth-terminal-copy"));

    await waitFor(() =>
      expect(sonner.toast.error).toHaveBeenCalledWith(
        "Failed to copy GitHub sign-in command",
      ),
    );
  });
});
