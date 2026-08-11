import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { usePersonaOverlayPreview } from "@/hooks/usePersonas";

import { getPersonaSkippedReasonCopy } from "./personaSkippedReason";

export interface PersonaInjectedPromptDialogProps {
  conversationId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * Shows the exact `<ralphx_agent_persona>` block the next send would inject.
 * The dialog shell paints immediately; the preview is fetched on open and
 * only ever travels through the direct command response (never events).
 */
export function PersonaInjectedPromptDialog({
  conversationId,
  open,
  onOpenChange,
}: PersonaInjectedPromptDialogProps) {
  const preview = usePersonaOverlayPreview(conversationId, open);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-w-2xl"
        aria-describedby="persona-injected-prompt-description"
      >
        <DialogHeader>
          <div>
            <DialogTitle>Injected persona — next send</DialogTitle>
            <DialogDescription id="persona-injected-prompt-description">
              {preview.data
                ? `${preview.data.slug} · v${preview.data.version} · applies to this conversation only`
                : "Exactly what the next message would inject"}
            </DialogDescription>
          </div>
        </DialogHeader>
        <div className="max-h-[60vh] overflow-y-auto px-6 pb-5">
          {preview.isPending ? (
            <div
              data-testid="persona-injected-prompt-loading"
              className="h-24 animate-pulse rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]"
            />
          ) : preview.isError ? (
            <div
              role="alert"
              className="rounded-md border border-[var(--status-error-border)] bg-[var(--status-error-muted)] px-3 py-2 text-sm text-[var(--status-error)]"
            >
              Could not load the injected prompt: {preview.error.message}
            </div>
          ) : preview.data === null ? (
            <p className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2 text-sm text-[var(--text-muted)]">
              No persona will be injected for the next send.
            </p>
          ) : (
            <>
              {preview.data.skippedReason && (
                <p
                  role="status"
                  className="mb-3 rounded-md border border-[var(--status-warning-border)] bg-[var(--status-warning-muted)] px-3 py-2 text-sm text-[var(--text-primary)]"
                >
                  {getPersonaSkippedReasonCopy(preview.data.skippedReason)}
                </p>
              )}
              {preview.data.renderedBlock && (
                <pre
                  data-testid="persona-injected-prompt-content"
                  className="whitespace-pre-wrap break-words rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2 font-mono text-xs leading-relaxed text-[var(--text-primary)]"
                >
                  {preview.data.renderedBlock}
                </pre>
              )}
              <p className="mt-3 text-xs text-[var(--text-muted)]">
                Persona guidance affects voice, priorities, and framing only; it
                never overrides safety or task instructions.
              </p>
            </>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
