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
  useGhAuthStatus: () => mocks.ghAuth,
  useLoginGhWithBrowser: () => mocks.loginGh,
  useSetupGhGitAuth: () => mocks.setupGh,
  useSwitchGitOriginToSsh: () => mocks.switchSsh,
  useResumeDeferredGitStartup: () => mocks.resumeDeferred,
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
    canSwitchToSsh: false,
    suggestedSshUrl: null,
  };
  mocks.diagnostics.isError = false;
  mocks.diagnostics.isLoading = false;
  mocks.diagnostics.refetch = vi
    .fn()
    .mockResolvedValue({ data: mocks.diagnostics.data, isError: false });
  mocks.ghAuth.data = false;
  mocks.ghAuth.isLoading = false;
  mocks.ghAuth.refetch = vi.fn().mockResolvedValue({ data: false });
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
    mocks.ghAuth.data = true;
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
    mocks.ghAuth.data = true;
    render(<GitAuthRepairPanel projectId="proj-1" />);
    await user.click(screen.getByTestId("git-auth-setup-gh"));
    expect(mocks.setupGh.mutateAsync).toHaveBeenCalled();
  });

  it("Recheck refetches diagnostics and gh auth status", async () => {
    const user = userEvent.setup();
    render(<GitAuthRepairPanel projectId="proj-1" />);
    await user.click(screen.getByTestId("git-auth-recheck"));
    expect(mocks.diagnostics.refetch).toHaveBeenCalled();
    expect(mocks.ghAuth.refetch).toHaveBeenCalled();
  });

  it("PR-only access mode shows the title 'GitHub PR Access'", () => {
    mocks.diagnostics.data = {
      fetchUrl: "git@github.com:owner/repo.git",
      pushUrl: "git@github.com:owner/repo.git",
      fetchKind: "SSH",
      pushKind: "SSH",
      mixedAuthModes: false,
      canSwitchToSsh: false,
      suggestedSshUrl: null,
    };
    mocks.ghAuth.data = false;

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
});
