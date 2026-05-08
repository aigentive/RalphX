import { Copy, Info } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";

export function GitAuthTerminalSetupButton({ onCopy }: { onCopy: () => void }) {
  return (
    <div className="flex items-center gap-1">
      <Button
        type="button"
        variant="secondary"
        size="sm"
        className="settings-btn-ghost"
        onClick={onCopy}
        data-testid="git-auth-copy-gh-login"
      >
        <Copy className="h-3.5 w-3.5" />
        Use Terminal
      </Button>
      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              aria-label="What does Use Terminal do?"
              className="inline-flex h-7 w-7 items-center justify-center rounded-md text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            >
              <Info className="h-3.5 w-3.5" />
            </button>
          </TooltipTrigger>
          <TooltipContent side="top" className="max-w-[260px]">
            Copies a command you can paste into Terminal. It signs in GitHub CLI in a
            way RalphX can see.
          </TooltipContent>
        </Tooltip>
      </TooltipProvider>
    </div>
  );
}
