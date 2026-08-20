/**
 * CloneAuthFailureCard - shown when a clone attempt fails with
 * CLONE_AUTH_FAILED. Reuses the existing GitHub-auth hooks; there is no
 * project id yet during a clone, so it never reaches for the project-scoped
 * auth diagnostics/SSH-switch surfaces.
 */

import { Button } from "@/components/ui/button";
import { KeyRound, Loader2, LogIn } from "lucide-react";
import { useGhAuthStatus, useLoginGhWithBrowser } from "@/hooks/useGithubSettings";

export interface CloneAuthFailureCardProps {
  suggestedSshUrl: string | null;
  onUseSshUrl: () => void;
  onRetry: () => void;
  canRetry: boolean;
}

export function CloneAuthFailureCard({
  suggestedSshUrl,
  onUseSshUrl,
  onRetry,
  canRetry,
}: CloneAuthFailureCardProps) {
  const authStatus = useGhAuthStatus();
  const loginMutation = useLoginGhWithBrowser();
  const isAuthenticated = authStatus.data === true;

  return (
    <div
      data-testid="clone-auth-card"
      className="space-y-3 rounded-lg border border-[var(--status-error)] bg-[var(--status-error-muted)] px-3 py-3"
    >
      <div className="flex items-start gap-2">
        <KeyRound className="h-4 w-4 mt-0.5 text-[var(--status-error)]" />
        <div className="space-y-1">
          <p className="text-sm text-[var(--status-error)]">
            RalphX couldn&apos;t authenticate with this repository.
          </p>
          <p className="text-xs text-[var(--text-muted)]">
            {isAuthenticated
              ? "GitHub is connected. Try cloning again, or use SSH below."
              : "Sign in with GitHub, then try again."}
          </p>
        </div>
      </div>
      <div className="flex flex-wrap gap-2">
        {!isAuthenticated && (
          <Button
            data-testid="clone-auth-login-button"
            type="button"
            size="sm"
            onClick={() => loginMutation.mutate()}
            disabled={loginMutation.isPending}
            className="gap-2 bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-primary)]/90"
          >
            {loginMutation.isPending && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            <LogIn className="h-3.5 w-3.5" />
            Sign in with GitHub
          </Button>
        )}
        {suggestedSshUrl && (
          <Button
            data-testid="clone-use-ssh-button"
            type="button"
            size="sm"
            variant="secondary"
            onClick={onUseSshUrl}
            className="bg-[var(--bg-elevated)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] border-0"
          >
            Use SSH instead
          </Button>
        )}
        <Button
          data-testid="clone-retry-button"
          type="button"
          size="sm"
          variant="ghost"
          onClick={onRetry}
          disabled={!canRetry}
          className="bg-[var(--bg-elevated)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
        >
          Retry
        </Button>
      </div>
    </div>
  );
}
