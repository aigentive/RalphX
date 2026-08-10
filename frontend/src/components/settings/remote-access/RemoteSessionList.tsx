/**
 * RemoteSessionList — live sessions with immediate disconnect, plus recent
 * audit entries (§5.4/§5.5). Session rows update live via the local-only
 * `remote:session_connected`/`remote:session_closed` events the parent
 * subscribes to.
 */

import { Unplug } from "lucide-react";

import type {
  RemoteAuditEntry,
  RemoteDeviceView,
  RemoteSessionView,
} from "@/api/remote-host";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { StatusPill } from "@/components/ui/status-pill";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { formatRelativeTime } from "@/lib/formatters";

import { RemoteAccessCardHeader, RemoteAccessSkeletonRows } from "./RemoteAccessSection";

export interface RemoteSessionListProps {
  devices: RemoteDeviceView[] | null;
  sessions: RemoteSessionView[] | null;
  auditEntries: RemoteAuditEntry[] | null;
  onDisconnect: (sessionId: string) => void;
}

/** "pairing_code_created" → "Pairing code created". */
function humanizeAuditAction(action: string): string {
  const words = action.split("_").join(" ");
  return words.charAt(0).toUpperCase() + words.slice(1);
}

export function RemoteSessionList({
  devices,
  sessions,
  auditEntries,
  onDisconnect,
}: RemoteSessionListProps) {
  const deviceNames = new Map(
    (devices ?? []).map((device) => [device.id, device.name]),
  );

  return (
    <Card className="bg-[var(--bg-elevated)] border-[var(--border-default)] shadow-[var(--shadow-xs)]">
      <RemoteAccessCardHeader
        title="Live sessions"
        description="Connections currently open against this Mac"
      />
      <div className="px-5 pb-5 space-y-4">
        {sessions === null ? (
          <RemoteAccessSkeletonRows rows={1} />
        ) : sessions.length === 0 ? (
          <p className="text-xs text-[var(--text-muted)]">No live sessions.</p>
        ) : (
          <ul className="space-y-1">
            {sessions.map((session) => (
              <li
                key={session.id}
                data-testid={`remote-session-${session.id}`}
                className="flex items-center justify-between gap-3 py-2.5 border-b border-[var(--border-subtle)] last:border-0"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2 min-w-0">
                    <span className="text-sm font-medium text-[var(--text-primary)] truncate">
                      {deviceNames.get(session.deviceId) ?? "Unknown device"}
                    </span>
                    <code className="font-mono text-xs text-[var(--text-muted)] shrink-0">
                      {session.remoteAddr}
                    </code>
                    {!session.live && (
                      <StatusPill tone="neutral" size="sm" label="Stale" />
                    )}
                  </div>
                  <p className="text-xs text-[var(--text-muted)] mt-0.5">
                    Connected {formatRelativeTime(session.connectedAt)} · last
                    active {formatRelativeTime(session.lastActiveAt)}
                  </p>
                </div>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon-sm"
                      data-testid={`remote-session-disconnect-${session.id}`}
                      aria-label={`Disconnect session from ${
                        deviceNames.get(session.deviceId) ?? "unknown device"
                      }`}
                      onClick={() => onDisconnect(session.id)}
                    >
                      <Unplug className="h-4 w-4" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>Disconnect session</TooltipContent>
                </Tooltip>
              </li>
            ))}
          </ul>
        )}

        <div>
          <p className="text-xs font-medium text-[var(--text-secondary)] mb-2">
            Recent activity
          </p>
          {auditEntries === null ? (
            <RemoteAccessSkeletonRows rows={1} />
          ) : auditEntries.length === 0 ? (
            <p className="text-xs text-[var(--text-muted)]">No activity yet.</p>
          ) : (
            <ul data-testid="remote-audit" className="space-y-1">
              {auditEntries.slice(0, 10).map((entry) => (
                <li
                  key={entry.id}
                  className="flex items-baseline justify-between gap-2 text-xs"
                >
                  <span className="min-w-0 truncate text-[var(--text-secondary)]">
                    {humanizeAuditAction(entry.action)}
                    {entry.deviceId && deviceNames.has(entry.deviceId)
                      ? ` · ${deviceNames.get(entry.deviceId)}`
                      : ""}
                    {entry.detail ? ` — ${entry.detail}` : ""}
                  </span>
                  <span className="shrink-0 text-[var(--text-muted)]">
                    {formatRelativeTime(entry.createdAt)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </Card>
  );
}
