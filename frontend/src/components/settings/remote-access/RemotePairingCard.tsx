/**
 * RemotePairingCard — "Pair a device" flow (§4.2, §5.4).
 *
 * Shows the freshly minted code exactly once: grouped for manual entry (R-12),
 * as a copyable `ralphx://pair` URL with the code in the hash fragment (§3.7),
 * under a live 10-minute countdown. Outstanding codes list with cancel covers
 * the §4.6 stolen-QR row.
 */

import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";

import type {
  MintedRemotePairingCode,
  RemotePairingCodeView,
} from "@/api/remote-host";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { CopyableRef } from "@/components/ui/copyable-ref";
import { NoticeBanner } from "@/components/ui/notice-banner";
import { StatusPill } from "@/components/ui/status-pill";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { formatDateTime } from "@/lib/formatters";

import { RemoteAccessCardHeader, RemoteAccessSkeletonRows } from "./RemoteAccessSection";
import {
  buildPairingUrl,
  formatCountdown,
  groupPairingCode,
  remainingSeconds,
} from "./remote-access-utils";

export interface RemotePairingCardProps {
  pairing: MintedRemotePairingCode | null;
  pairingBusy: boolean;
  listenerEnabled: boolean;
  preferredEndpoint: string | null;
  outstandingCodes: RemotePairingCodeView[] | null;
  onGenerate: () => void;
  onCancel: (id: string) => void;
  onExpired: () => void;
}

/** Ticks once a second while a code is displayed; freezes when there is none. */
function useCountdown(pairing: MintedRemotePairingCode | null): number {
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    if (!pairing) {
      return;
    }
    setNowMs(Date.now());
    const interval = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(interval);
  }, [pairing]);
  return pairing ? remainingSeconds(pairing.expiresAt, nowMs) : 0;
}

export function RemotePairingCard({
  pairing,
  pairingBusy,
  listenerEnabled,
  preferredEndpoint,
  outstandingCodes,
  onGenerate,
  onCancel,
  onExpired,
}: RemotePairingCardProps) {
  const remaining = useCountdown(pairing);
  const expired = pairing !== null && remaining <= 0;

  // Fire onExpired exactly once per code as its countdown crosses zero.
  const notifiedExpiryFor = useRef<string | null>(null);
  useEffect(() => {
    if (expired && pairing && notifiedExpiryFor.current !== pairing.id) {
      notifiedExpiryFor.current = pairing.id;
      onExpired();
    }
  }, [expired, pairing, onExpired]);

  const grouped = pairing ? groupPairingCode(pairing.code) : null;
  const pairingUrl =
    pairing && preferredEndpoint
      ? buildPairingUrl(preferredEndpoint, pairing.code)
      : null;

  return (
    <Card className="bg-[var(--bg-elevated)] border-[var(--border-default)] shadow-[var(--shadow-xs)]">
      <RemoteAccessCardHeader
        title="Pair a device"
        description="Single-use code, 10-minute expiry — shown only once"
      />
      <div className="px-5 pb-5 space-y-4">
        {!pairing || expired ? (
          <div className="flex items-center justify-between gap-3">
            <p className="text-xs text-[var(--text-muted)]">
              {expired
                ? "That code expired before it was used. Generate a new one."
                : "Generate a code, then scan or paste it on the device you are pairing."}
            </p>
            <Button
              data-testid="remote-pair-device"
              size="sm"
              onClick={onGenerate}
              disabled={pairingBusy || !listenerEnabled}
              className="bg-[var(--accent-primary)] hover:bg-[var(--accent-hover)] text-[var(--text-on-accent)] shrink-0"
            >
              {pairingBusy ? "Generating…" : "Generate pairing code"}
            </Button>
          </div>
        ) : null}
        {!listenerEnabled && !pairing && (
          <p className="text-xs text-[var(--text-muted)]">
            Enable remote access first — a device cannot redeem a code while the
            listener is off.
          </p>
        )}
        {expired && (
          <NoticeBanner tone="warning" testId="remote-pairing-expired">
            Pairing code expired. Expired codes are single-use and can no longer be
            redeemed.
          </NoticeBanner>
        )}

        {pairing && !expired && grouped && (
          <div
            data-testid="remote-pairing-card"
            className="rounded-md p-4 space-y-3"
            style={{
              backgroundColor: "var(--bg-surface, #1e1e23)",
              borderColor: "var(--border-default, #393940)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
          >
            <div className="flex items-center justify-between gap-3">
              <StatusPill
                tone="accent"
                size="sm"
                testId="remote-pairing-countdown"
                label={`Expires in ${formatCountdown(remaining)}`}
              />
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    data-testid="remote-pairing-cancel"
                    aria-label="Cancel pairing code"
                    onClick={() => onCancel(pairing.id)}
                  >
                    <X className="h-4 w-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>Cancel pairing code</TooltipContent>
              </Tooltip>
            </div>

            {/* QR: deferred — no QR library exists in frontend/package.json and PR 1.7
                does not add dependencies without a bundle-convention review. The manual
                block below is the R-12 fallback; the QR would encode pairingUrl. */}
            <div className="space-y-1">
              <p className="text-xs text-[var(--text-muted)]">
                Enter this code on the device
              </p>
              <p
                data-testid="remote-pairing-code"
                className="font-mono text-lg tracking-wide text-[var(--text-primary)] select-all"
              >
                <span className="text-[var(--text-muted)]">{grouped.prefix}</span>
                {grouped.groups.map((group, index) => (
                  <span key={index}>
                    {index > 0 && <span className="text-[var(--text-muted)]"> </span>}
                    {group}
                  </span>
                ))}
              </p>
              <CopyableRef
                value={pairing.code}
                ariaLabel="Copy pairing code"
                testId="remote-pairing-code-copy"
              />
            </div>

            <div className="space-y-1">
              <p className="text-xs text-[var(--text-muted)]">
                Or open this link on the device
              </p>
              {pairingUrl ? (
                <CopyableRef
                  value={pairingUrl}
                  ariaLabel="Copy pairing URL"
                  testId="remote-pairing-url"
                />
              ) : (
                <p
                  data-testid="remote-pairing-url-unavailable"
                  className="text-xs text-[var(--text-muted)]"
                >
                  No advertised endpoint is known yet — enter the host address and
                  the code manually on the device.
                </p>
              )}
            </div>
          </div>
        )}

        <div>
          <p className="text-xs font-medium text-[var(--text-secondary)] mb-2">
            Outstanding codes
          </p>
          {outstandingCodes === null ? (
            <RemoteAccessSkeletonRows rows={1} />
          ) : outstandingCodes.length === 0 ? (
            <p className="text-xs text-[var(--text-muted)]">
              No outstanding pairing codes.
            </p>
          ) : (
            <ul data-testid="remote-outstanding-codes" className="space-y-1.5">
              {outstandingCodes.map((code) => (
                <li
                  key={code.id}
                  className="flex items-center justify-between gap-2 text-xs text-[var(--text-secondary)]"
                >
                  <span className="min-w-0 truncate">
                    Created {formatDateTime(code.createdAt)} · expires{" "}
                    {formatDateTime(code.expiresAt)}
                  </span>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-sm"
                        data-testid={`remote-code-cancel-${code.id}`}
                        aria-label="Cancel outstanding pairing code"
                        onClick={() => onCancel(code.id)}
                      >
                        <X className="h-4 w-4" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>Cancel outstanding pairing code</TooltipContent>
                  </Tooltip>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </Card>
  );
}
