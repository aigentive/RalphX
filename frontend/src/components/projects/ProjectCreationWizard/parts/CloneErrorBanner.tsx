/**
 * CloneErrorBanner - shared plain-language error banner for CloneStep and
 * CloneConfigureForm. No git commands, ref syntax, or raw backend codes are
 * ever rendered on screen.
 */

import { AlertTriangle } from "lucide-react";

export function ErrorBanner({ text, testId }: { text: string; testId?: string }) {
  return (
    <div
      data-testid={testId}
      className="flex items-center gap-2 px-3 py-2 rounded-lg bg-[var(--status-error-muted)] text-[var(--status-error)]"
    >
      <AlertTriangle className="h-3.5 w-3.5" />
      <span className="text-sm">{text}</span>
    </div>
  );
}
