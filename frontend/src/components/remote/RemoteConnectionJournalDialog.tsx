/**
 * The user-readable connection log for one remote environment.
 *
 * A pure READER of `remoteConnectionJournalStore` (the environment runtime is the
 * single writer). Exists so "Reconnecting…" is never a dead end: the dialog shows
 * WHICH step failed — descriptor, socket, permissions, or a named hydration query —
 * in plain words, newest first.
 */

import { useMemo } from "react";

import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  useRemoteConnectionJournalStore,
  type ConnectionJournalEntry,
  type ConnectionJournalKind,
} from "@/stores/remoteConnectionJournalStore";

const KIND_LABELS: Record<ConnectionJournalKind, string> = {
  state: "state",
  attempt: "attempt",
  barrier: "hydration",
  stream: "stream",
  info: "info",
  action: "you",
};

const KIND_CLASSES: Record<ConnectionJournalKind, string> = {
  state: "text-text-secondary",
  attempt: "text-[var(--status-warning)]",
  barrier: "text-[var(--status-error)]",
  stream: "text-[var(--status-warning)]",
  info: "text-text-muted",
  action: "text-accent-primary",
};

function formatTimestamp(at: string): string {
  const parsed = new Date(at);
  if (Number.isNaN(parsed.getTime())) {
    return at;
  }
  return parsed.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });
}

function journalAsText(entries: readonly ConnectionJournalEntry[]): string {
  return entries
    .map(
      (entry) =>
        `${entry.at} [${KIND_LABELS[entry.kind]}] ${entry.message}${entry.detail ? ` — ${entry.detail}` : ""}`
    )
    .join("\n");
}

async function copyJournal(text: string): Promise<void> {
  try {
    if (!navigator.clipboard) {
      throw new Error("clipboard unavailable");
    }
    await navigator.clipboard.writeText(text);
    toast.success("Connection log copied");
  } catch {
    // WKWebView rejects the async clipboard API inside focus-trapped dialogs;
    // the selection-based path still works there.
    try {
      const area = document.createElement("textarea");
      area.value = text;
      area.setAttribute("readonly", "");
      area.style.position = "fixed";
      area.style.opacity = "0";
      document.body.appendChild(area);
      area.select();
      const copied = document.execCommand("copy");
      area.remove();
      if (!copied) {
        throw new Error("execCommand copy refused");
      }
      toast.success("Connection log copied");
    } catch {
      toast.error("Failed to copy the connection log");
    }
  }
}

export function RemoteConnectionJournalDialog({
  environmentId,
  environmentName,
  open,
  onOpenChange,
}: {
  environmentId: string;
  environmentName: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const entries = useRemoteConnectionJournalStore(
    (state) => state.journals[environmentId]
  );
  const newestFirst = useMemo(() => [...(entries ?? [])].reverse(), [entries]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-w-[min(56rem,92vw)]"
        data-testid="remote-connection-journal-dialog"
      >
        <DialogHeader>
          <DialogTitle>{`Connection log — ${environmentName}`}</DialogTitle>
          <DialogDescription>
            What this device observed while talking to the host, newest first.
          </DialogDescription>
        </DialogHeader>
        <div className="flex items-center justify-end">
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 px-2 text-xs"
            data-testid="remote-connection-journal-copy"
            disabled={newestFirst.length === 0}
            onClick={() => void copyJournal(journalAsText(newestFirst))}
          >
            Copy log
          </Button>
        </div>
        <div className="max-h-[70vh] overflow-y-auto pr-1">
          {newestFirst.length === 0 ? (
            <p className="py-6 text-center text-sm text-text-muted">
              No connection events recorded yet for this environment.
            </p>
          ) : (
            <ul className="flex flex-col gap-1.5">
              {newestFirst.map((entry, index) => (
                <li
                  key={`${entry.at}-${index}`}
                  className="rounded-md bg-bg-elevated px-2.5 py-1.5"
                  data-testid="remote-connection-journal-entry"
                >
                  <div className="flex items-baseline gap-2">
                    <span className="shrink-0 font-mono text-[11px] tabular-nums text-text-muted">
                      {formatTimestamp(entry.at)}
                    </span>
                    <span
                      className={`shrink-0 text-[11px] uppercase tracking-wide ${KIND_CLASSES[entry.kind]}`}
                    >
                      {KIND_LABELS[entry.kind]}
                    </span>
                    <span className="min-w-0 text-sm text-text-primary">
                      {entry.message}
                    </span>
                  </div>
                  {entry.detail ? (
                    <p className="mt-0.5 break-words pl-1 font-mono text-[11px] leading-4 text-text-secondary">
                      {entry.detail}
                    </p>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
