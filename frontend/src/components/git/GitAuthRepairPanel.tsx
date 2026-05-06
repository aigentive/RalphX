import { listen } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  CheckCircle2,
  GitBranch,
  KeyRound,
  Loader2,
  RefreshCw,
} from "lucide-react";
import { useState, type ReactNode } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { useConfirmation } from "@/hooks/useConfirmation";
import {
  useGhAuthStatus,
  useGitAuthDiagnostics,
  useLoginGhWithBrowser,
  useResumeDeferredGitStartup,
  useSetupGhGitAuth,
  useSwitchGitOriginToSsh,
} from "@/hooks/useGithubSettings";
import type { GitAuthDiagnostics } from "@/hooks/useGithubSettings";
import { GhAuthLoginPrompt, type GhAuthLoginPromptPayload } from "./GhAuthLoginPrompt";
import { GitAuthTerminalSetupButton } from "./GitAuthTerminalSetupButton";

function authModeLabel(fetchKind: string | null | undefined, pushKind: string | null | undefined) {
  if (!fetchKind && !pushKind) {
    return "No origin";
  }
  return `Fetch ${fetchKind ?? "unknown"} / Push ${pushKind ?? "unknown"}`;
}

function errorMessage(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : fallback;
}

function isGithubHttpsRemote(url: string | null | undefined) {
  return url?.trim().startsWith("https://github.com/") ?? false;
}

function hasDeferredStartupBlockingIssue(
  diagnostics: GitAuthDiagnostics | undefined,
  ghAuthenticated: boolean | undefined,
  requiresGhAuth: boolean,
  diagnosticsFailed = false,
) {
  if (diagnosticsFailed) {
    return true;
  }
  if (!diagnostics) {
    return false;
  }
  if (diagnostics.mixedAuthModes) {
    return true;
  }
  return Boolean(
    ghAuthenticated === false &&
      (requiresGhAuth ||
        isGithubHttpsRemote(diagnostics.fetchUrl) ||
        isGithubHttpsRemote(diagnostics.pushUrl)),
  );
}

const APP_LIKE_GH_LOGIN_COMMAND =
  'APP_PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin" && env -i HOME="$HOME" PATH="$APP_PATH" gh auth login --hostname github.com --git-protocol ssh --web --skip-ssh-key';
const GH_AUTH_LOGIN_PROMPT_EVENT = "gh-auth:login_prompt";

export function GitAuthRepairPanel({
  projectId,
  surface = "settings",
  showWhenHealthy = false,
  requiresGhAuth = false,
}: {
  projectId: string | null;
  surface?: "settings" | "publish";
  showWhenHealthy?: boolean;
  requiresGhAuth?: boolean;
}) {
  const diagnosticsQuery = useGitAuthDiagnostics(projectId);
  const ghAuthQuery = useGhAuthStatus();
  const switchToSshMutation = useSwitchGitOriginToSsh();
  const loginGhWithBrowserMutation = useLoginGhWithBrowser();
  const setupGhGitAuthMutation = useSetupGhGitAuth();
  const resumeDeferredGitStartupMutation = useResumeDeferredGitStartup();
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const [loginPrompt, setLoginPrompt] = useState<GhAuthLoginPromptPayload | null>(null);

  if (!projectId) {
    return null;
  }

  const diagnostics = diagnosticsQuery.data;
  const isGhAuthed = ghAuthQuery.data === true;
  const isGhMissing = ghAuthQuery.data === false;
  const isChecking = diagnosticsQuery.isLoading || ghAuthQuery.isLoading;
  const hasHttpsRemote =
    diagnostics?.fetchKind === "HTTPS" || diagnostics?.pushKind === "HTTPS";
  const hasGithubHttpsRemote =
    isGithubHttpsRemote(diagnostics?.fetchUrl) ||
    isGithubHttpsRemote(diagnostics?.pushUrl);
  const canSetupGithubHttps = isGhAuthed && hasGithubHttpsRemote;
  const canLoginGithubCli = isGhMissing && (requiresGhAuth || hasGithubHttpsRemote);
  const canCopyGithubLogin = canLoginGithubCli;
  const hasRepairAction =
    Boolean(diagnostics?.canSwitchToSsh) ||
    canSetupGithubHttps ||
    canLoginGithubCli ||
    canCopyGithubLogin;
  const hasGitTransportIssue =
    diagnosticsQuery.isError ||
    ghAuthQuery.isError ||
    diagnostics?.mixedAuthModes ||
    hasHttpsRemote;
  const hasGhPrIssue = requiresGhAuth && isGhMissing;
  const hasVisibleIssue = hasGitTransportIssue || hasGhPrIssue;
  const showPrAccessMode = hasGhPrIssue && !hasGitTransportIssue;

  if (!showWhenHealthy && !isChecking && !hasVisibleIssue && !hasRepairAction) {
    return null;
  }

  const title = showPrAccessMode ? "GitHub PR Access" : "Git & GitHub Access";
  const subtitle = showPrAccessMode
    ? "Draft PR operations"
    : authModeLabel(diagnostics?.fetchKind, diagnostics?.pushKind);

  const messages: ReactNode[] = [];
  if (diagnosticsQuery.isError) {
    messages.push("Could not inspect the git origin for this project.");
  }
  if (diagnostics?.mixedAuthModes) {
    messages.push("Fetch and push use different auth modes. Background fetches can fail even when terminal pushes work.");
  }
  if (showPrAccessMode) {
    messages.push(
      "Git SSH access is ready. Sign in to GitHub CLI from RalphX before creating or updating draft PRs.",
    );
  } else if (isGhMissing && hasGithubHttpsRemote) {
    messages.push("GitHub CLI is not signed in for this app. Sign in from RalphX, then configure HTTPS credentials or switch origin to SSH.");
  }
  if (canSetupGithubHttps) {
    messages.push("HTTPS remotes need a non-interactive credential. Configure GitHub CLI credentials or switch origin to SSH.");
  } else if (hasHttpsRemote) {
    messages.push("HTTPS remotes need a non-interactive credential before background fetch or push can run.");
  }
  if (canCopyGithubLogin && !showPrAccessMode) {
    messages.push("HTTPS is still available: sign in to GitHub CLI, then let RalphX configure Git credentials.");
  }
  if (messages.length === 0 && !isChecking) {
    messages.push("Git remote auth and GitHub CLI status look ready.");
  }

  const handleRecheck = async () => {
    const [diagnosticsResult, ghAuthResult] = await Promise.all([
      diagnosticsQuery.refetch(),
      ghAuthQuery.refetch(),
    ]);
    return {
      diagnostics: diagnosticsResult.data,
      ghAuthenticated: ghAuthResult.data,
      diagnosticsFailed: diagnosticsResult.isError,
    };
  };

  const resumeDeferredStartupIfHealthy = async () => {
    const current = await handleRecheck();
    if (
      hasDeferredStartupBlockingIssue(
        current.diagnostics,
        current.ghAuthenticated,
        requiresGhAuth,
        current.diagnosticsFailed,
      )
    ) {
      return;
    }

    const resumed = await resumeDeferredGitStartupMutation.mutateAsync();
    if (resumed) {
      toast.success("Deferred startup recovery resumed");
    }
  };

  const handleCopyGithubHttpsSetup = async () => {
    try {
      await navigator.clipboard.writeText(APP_LIKE_GH_LOGIN_COMMAND);
      toast.success("Terminal sign-in command copied");
    } catch {
      toast.error("Failed to copy GitHub sign-in command");
    }
  };

  const mergeLoginPrompt = (prompt: GhAuthLoginPromptPayload) => {
    setLoginPrompt((current) => ({
      code: prompt.code ?? current?.code ?? null,
      url: prompt.url ?? current?.url ?? null,
    }));
  };

  const handleLoginGhWithBrowser = async () => {
    setLoginPrompt(null);
    let unlisten: (() => void) | undefined;

    try {
      unlisten = await listen<GhAuthLoginPromptPayload>(
        GH_AUTH_LOGIN_PROMPT_EVENT,
        (event) => mergeLoginPrompt(event.payload),
      );
      await loginGhWithBrowserMutation.mutateAsync();
      toast.success("GitHub CLI signed in");
      await resumeDeferredStartupIfHealthy();
    } catch (error) {
      toast.error(errorMessage(error, "Failed to sign in to GitHub CLI"));
    } finally {
      unlisten?.();
    }
  };

  const handleSwitchToSsh = async () => {
    const suggestedUrl = diagnostics?.suggestedSshUrl ?? "the SSH origin URL";
    const confirmed = await confirm({
      title: "Switch origin to SSH?",
      description: `This updates this project's origin fetch and push URLs to ${suggestedUrl}. Future git operations for this checkout will use SSH keys.`,
      confirmText: "Use SSH",
    });
    if (!confirmed) {
      return;
    }

    try {
      await switchToSshMutation.mutateAsync({ projectId });
      toast.success("Git origin switched to SSH");
      await resumeDeferredStartupIfHealthy();
    } catch (error) {
      toast.error(errorMessage(error, "Failed to switch git origin to SSH"));
    }
  };

  const handleSetupGhGitAuth = async () => {
    const confirmed = await confirm({
      title: "Set up GitHub HTTPS credentials?",
      description:
        "This runs gh auth setup-git with the app's resolved GitHub CLI and updates git credential configuration for GitHub HTTPS remotes.",
      confirmText: "Setup credentials",
    });
    if (!confirmed) {
      return;
    }

    try {
      await setupGhGitAuthMutation.mutateAsync();
      toast.success("GitHub HTTPS credentials configured");
      await resumeDeferredStartupIfHealthy();
    } catch (error) {
      toast.error(errorMessage(error, "Failed to configure GitHub HTTPS credentials"));
    }
  };

  const panelClassName =
    surface === "publish"
      ? "rounded-lg border p-4"
      : "settings-diag-card";

  return (
    <div
      className={panelClassName}
      style={
        surface === "publish"
          ? {
              backgroundColor: "var(--bg-surface)",
              borderColor: "var(--border-subtle)",
            }
          : undefined
      }
      data-testid="git-auth-repair-panel"
    >
      <div className="settings-diag-card__row">
        <div className="settings-diag-card__title-wrap">
          {isChecking ? (
            <Loader2 className="settings-diag-card__icon h-4 w-4 animate-spin" />
          ) : hasVisibleIssue ? (
            <AlertTriangle className="settings-diag-card__icon h-4 w-4" />
          ) : (
            <CheckCircle2 className="settings-diag-card__icon settings-diag-card__icon--ok h-4 w-4" />
          )}
          <div className="min-w-0">
            <div className="settings-diag-card__title">
              {title}
            </div>
            <div className="settings-diag-card__sub truncate">
              {isChecking
                ? "Checking repository credentials..."
                : subtitle}
            </div>
          </div>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="settings-btn-ghost"
          onClick={() => void resumeDeferredStartupIfHealthy()}
          disabled={isChecking || resumeDeferredGitStartupMutation.isPending}
          data-testid="git-auth-recheck"
        >
          <RefreshCw className="h-3.5 w-3.5" />
          Recheck
        </Button>
      </div>

      <div className="settings-diag-card__body">
        {messages.map((message, index) => (
          <p key={index}>{message}</p>
        ))}
        {loginPrompt && <GhAuthLoginPrompt prompt={loginPrompt} />}
        {diagnosticsQuery.isError && (
          <div className="text-[var(--status-warning)]">
            {errorMessage(diagnosticsQuery.error, "Git diagnostics failed")}
          </div>
        )}
      </div>

      {(diagnostics?.canSwitchToSsh ||
        canLoginGithubCli ||
        canSetupGithubHttps ||
        canCopyGithubLogin) && (
        <div className="settings-diag-card__actions">
          {diagnostics?.canSwitchToSsh && (
            <Button
              type="button"
              size="sm"
              className="settings-btn-primary"
              onClick={() => void handleSwitchToSsh()}
              disabled={switchToSshMutation.isPending}
              data-testid="git-auth-switch-ssh"
            >
              {switchToSshMutation.isPending ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <GitBranch className="h-3.5 w-3.5" />
              )}
              Use SSH
            </Button>
          )}
          {canLoginGithubCli && (
            <Button
              type="button"
              variant={showPrAccessMode ? "default" : "secondary"}
              size="sm"
              className="settings-btn-ghost"
              onClick={() => void handleLoginGhWithBrowser()}
              disabled={loginGhWithBrowserMutation.isPending}
              data-testid="git-auth-login-gh"
            >
              {loginGhWithBrowserMutation.isPending ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <KeyRound className="h-3.5 w-3.5" />
              )}
              Sign in
            </Button>
          )}
          {canSetupGithubHttps && (
            <Button
              type="button"
              variant="secondary"
              size="sm"
              className="settings-btn-ghost"
              onClick={() => void handleSetupGhGitAuth()}
              disabled={setupGhGitAuthMutation.isPending}
              data-testid="git-auth-setup-gh"
            >
              {setupGhGitAuthMutation.isPending ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <KeyRound className="h-3.5 w-3.5" />
              )}
              Setup HTTPS
            </Button>
          )}
          {canCopyGithubLogin && (
            <GitAuthTerminalSetupButton
              onCopy={() => void handleCopyGithubHttpsSetup()}
            />
          )}
        </div>
      )}

      <ConfirmationDialog {...confirmationDialogProps} />
    </div>
  );
}
