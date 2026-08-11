import { AlertCircle, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

export interface ErrorBannerProps {
  error: string;
  onDismiss?: () => void;
}

export function ErrorBanner({ error, onDismiss }: ErrorBannerProps) {
  return (
    <div
      role="alert"
      className="mx-6 mt-4 flex items-center gap-3 rounded-lg border border-[var(--status-error-border)] bg-[var(--status-error-muted)] p-3"
    >
      <AlertCircle
        aria-hidden="true"
        className="h-4 w-4 shrink-0 text-[var(--status-error)]"
      />
      <p className="flex-1 text-sm text-[var(--status-error)]">{error}</p>
      {onDismiss && (
        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                aria-label="Dismiss error"
                onClick={onDismiss}
                className="h-6 w-6 hover:bg-[var(--status-error-border)]"
              >
                <X
                  aria-hidden="true"
                  className="h-4 w-4 text-[var(--status-error)]"
                />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Dismiss error</TooltipContent>
          </Tooltip>
        </TooltipProvider>
      )}
    </div>
  );
}
