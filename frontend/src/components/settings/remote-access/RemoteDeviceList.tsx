/**
 * RemoteDeviceList — paired devices with the per-device agent-control revoke/re-grant toggle
 * and immediate device revoke.
 *
 * Re-granting agent control surfaces the explicit warning BEFORE committing: it
 * names kanban operation, agent start/steer, content editing, and tool-use
 * approval — host code execution. Withdrawal commits immediately (narrowing
 * needs no consent) and tears down the device's live sessions.
 */

import { useState } from "react";
import { Trash2 } from "lucide-react";

import type { RemoteDeviceView } from "@/api/remote-host";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { StatusPill } from "@/components/ui/status-pill";
import { Switch } from "@/components/ui/switch";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { formatDate, formatRelativeTime } from "@/lib/formatters";

import { RemoteAccessCardHeader, RemoteAccessSkeletonRows } from "./RemoteAccessSection";

export interface RemoteDeviceListProps {
  devices: RemoteDeviceView[] | null;
  onSetAgentControl: (deviceId: string, enabled: boolean) => void;
  onRevoke: (deviceId: string) => void;
}

export function RemoteDeviceList({
  devices,
  onSetAgentControl,
  onRevoke,
}: RemoteDeviceListProps) {
  const [grantCandidate, setGrantCandidate] = useState<RemoteDeviceView | null>(null);
  const [revokeCandidate, setRevokeCandidate] = useState<RemoteDeviceView | null>(null);

  const handleToggle = (device: RemoteDeviceView, enabled: boolean) => {
    if (enabled) {
      // Granting ui:agent requires the explicit warning first — no invoke yet.
      setGrantCandidate(device);
    } else {
      // Narrowing commits immediately and fires the device's kill channels.
      onSetAgentControl(device.id, false);
    }
  };

  return (
    <Card className="bg-[var(--bg-elevated)] border-[var(--border-default)] shadow-[var(--shadow-xs)]">
      <RemoteAccessCardHeader
        title="Paired devices"
        description="Agent control starts on; revoke or re-grant it for each device"
      />
      <div className="px-5 pb-5">
        {devices === null ? (
          <RemoteAccessSkeletonRows rows={2} />
        ) : devices.length === 0 ? (
          <p className="text-xs text-[var(--text-muted)]">
            No devices paired yet. Generate a pairing code above to add one.
          </p>
        ) : (
          <ul className="space-y-1">
            {devices.map((device) => {
              const revoked = device.revokedAt !== null;
              return (
                <li
                  key={device.id}
                  data-testid={`remote-device-${device.id}`}
                  className="flex items-center justify-between gap-3 py-3 border-b border-[var(--border-subtle)] last:border-0"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2 min-w-0">
                      <span className="text-sm font-medium text-[var(--text-primary)] truncate">
                        {device.name}
                      </span>
                      <code className="font-mono text-xs text-[var(--text-muted)] shrink-0">
                        {device.tokenPrefix}…
                      </code>
                      {revoked && <StatusPill tone="error" size="sm" label="Revoked" />}
                      {!revoked && device.liveSessionCount > 0 && (
                        <StatusPill
                          tone="success"
                          size="sm"
                          label={`${device.liveSessionCount} live`}
                        />
                      )}
                    </div>
                    <p className="text-xs text-[var(--text-muted)] mt-0.5">
                      Paired {formatDate(device.createdAt)} · last seen{" "}
                      {device.lastSeenAt ? formatRelativeTime(device.lastSeenAt) : "never"}
                    </p>
                  </div>

                  {!revoked && (
                    <div className="flex items-center gap-3 shrink-0">
                      <div className="flex items-center gap-2">
                        <span className="text-xs text-[var(--text-secondary)]">
                          Agent control
                        </span>
                        <Switch
                          data-testid={`remote-agent-control-${device.id}`}
                          checked={device.agentControlGranted}
                          onCheckedChange={(checked) => handleToggle(device, checked)}
                          aria-label={`Allow remote agent control for ${device.name}`}
                          className="data-[state=checked]:bg-[var(--accent-primary)]"
                        />
                      </div>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon-sm"
                            data-testid={`remote-device-revoke-${device.id}`}
                            aria-label={`Revoke ${device.name}`}
                            onClick={() => setRevokeCandidate(device)}
                            className="text-[var(--status-error)] hover:text-[var(--status-error)]"
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>Revoke device</TooltipContent>
                      </Tooltip>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </div>

      {/* Grant warning — the one deliberate consent (§5.4). */}
      <AlertDialog
        open={grantCandidate !== null}
        onOpenChange={(open) => {
          if (!open) {
            setGrantCandidate(null);
          }
        }}
      >
        <AlertDialogContent data-testid="remote-agent-warning">
          <AlertDialogHeader>
            <AlertDialogTitle>
              Allow remote agent control
              {grantCandidate ? ` for ${grantCandidate.name}` : ""}?
            </AlertDialogTitle>
            <AlertDialogDescription asChild>
              <div className="space-y-2 text-sm">
                <p>This device will be able to:</p>
                <ul className="list-disc pl-5 space-y-1">
                  <li>
                    Operate the kanban — move, approve, resume, and unblock tasks,
                    which auto-spawns agents
                  </li>
                  <li>Inject tasks into the ready queue</li>
                  <li>Start and steer agents on this Mac</li>
                  <li>
                    Edit task titles, descriptions, notes, plans, steps, proposals,
                    and artifacts
                  </li>
                  <li>
                    Approve tool use — that is code execution on this Mac
                  </li>
                </ul>
                <p>
                  Only enable this for a device you fully control. You can withdraw
                  it at any time; withdrawal disconnects the device&apos;s live
                  sessions immediately.
                </p>
              </div>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel data-testid="remote-agent-warning-cancel">
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              data-testid="remote-agent-warning-confirm"
              onClick={() => {
                if (grantCandidate) {
                  onSetAgentControl(grantCandidate.id, true);
                }
                setGrantCandidate(null);
              }}
              className="bg-[var(--accent-primary)] hover:bg-[var(--accent-hover)] text-[var(--text-on-accent)]"
            >
              Allow agent control
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* Revoke confirm — irreversible, so one short confirmation. */}
      <AlertDialog
        open={revokeCandidate !== null}
        onOpenChange={(open) => {
          if (!open) {
            setRevokeCandidate(null);
          }
        }}
      >
        <AlertDialogContent data-testid="remote-device-revoke-confirm">
          <AlertDialogHeader>
            <AlertDialogTitle>
              Revoke {revokeCandidate?.name ?? "this device"}?
            </AlertDialogTitle>
            <AlertDialogDescription>
              Its token stops working and its live sessions are disconnected
              immediately. Revocation cannot be undone — pair the device again to
              restore access.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel data-testid="remote-device-revoke-cancel">
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              data-testid="remote-device-revoke-confirm-action"
              onClick={() => {
                if (revokeCandidate) {
                  onRevoke(revokeCandidate.id);
                }
                setRevokeCandidate(null);
              }}
              className="bg-[var(--status-error)] hover:bg-[var(--status-error)] text-white"
            >
              Revoke device
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Card>
  );
}
