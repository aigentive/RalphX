import { ExternalLink } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Button } from "@/components/ui/button";

export interface GhAuthLoginPromptPayload {
  code?: string | null;
  url?: string | null;
}

export function GhAuthLoginPrompt({ prompt }: { prompt: GhAuthLoginPromptPayload }) {
  if (!prompt.code && !prompt.url) {
    return null;
  }

  return (
    <div
      className="mt-2 rounded-md border px-3 py-2 text-xs"
      style={{
        background: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        color: "var(--text-secondary)",
      }}
      data-testid="gh-auth-login-prompt"
    >
      <div className="flex flex-wrap items-center gap-2">
        {prompt.code && (
          <>
            <span>Enter this GitHub code:</span>
            <span className="rounded bg-[var(--bg-subtle)] px-2 py-1 font-mono text-[var(--text-primary)]">
              {prompt.code}
            </span>
          </>
        )}
        {prompt.url && (
          <Button
            type="button"
            variant="secondary"
            size="sm"
            className="h-7 gap-1.5 px-2 text-[0.6875rem]"
            onClick={() => void openUrl(prompt.url!)}
            data-testid="gh-auth-open-github"
          >
            <ExternalLink className="h-3.5 w-3.5" />
            Open GitHub
          </Button>
        )}
      </div>
    </div>
  );
}
