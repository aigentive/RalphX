import { AlertTriangle } from "lucide-react";

export interface PersonaUnavailableNoticeProps {
  message: string;
  onRemoveAndRetry: () => void;
  onOpenPersonas: () => void;
  disabled?: boolean;
}

export function PersonaUnavailableNotice({
  message,
  onRemoveAndRetry,
  onOpenPersonas,
  disabled = false,
}: PersonaUnavailableNoticeProps) {
  return (
    <div
      role="alert"
      data-testid="persona-unavailable-notice"
      className="mx-auto flex max-w-[620px] flex-col items-start gap-2 rounded-md border border-[var(--status-warning-border)] bg-[var(--status-warning-muted)] px-4 py-3 text-left text-[0.8125rem] text-[var(--status-warning)]"
    >
      <div className="flex items-start gap-2">
        <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
        <div>
          <p className="font-medium leading-snug">Persona unavailable: {message}</p>
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-2 pl-6">
        <button
          type="button"
          disabled={disabled}
          onClick={onRemoveAndRetry}
          className="rounded-md bg-[var(--accent-muted)] px-3 py-1.5 text-[0.75rem] font-medium text-[var(--accent-primary)] disabled:cursor-not-allowed disabled:opacity-60"
        >
          Remove persona and retry
        </button>
        <button
          type="button"
          onClick={onOpenPersonas}
          className="rounded-md px-2 py-1.5 text-[0.75rem] font-medium text-[var(--accent-primary)] hover:bg-[var(--accent-muted)]"
        >
          Manage personas <span aria-hidden="true">→</span>
        </button>
      </div>
    </div>
  );
}
