/**
 * RemotePairingCard — "Pair a device" flow (§4.2, §5.4).
 *
 * Shows the freshly minted code exactly once: grouped for manual entry (R-12),
 * as a QR and a copyable `ralphx://pair` URL with the code in the hash fragment
 * (§3.7), under a live 10-minute countdown. Outstanding codes list with cancel
 * covers the §4.6 stolen-QR row.
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
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { formatDateTime } from "@/lib/formatters";
import {
  encodeQrMatrix,
  qrMatrixToPath,
  qrSvgExtent,
} from "@/lib/qr/qr-encode";

import {
  cancelScheduledJob,
  scheduleAfterPaint,
} from "../SettingsDialog.performance";
import {
  RemoteAccessCardHeader,
  RemoteAccessSkeletonRows,
} from "./RemoteAccessSection";
import {
  buildPairingUrl,
  formatCountdown,
  groupPairingCode,
  remainingSeconds,
} from "./remote-access-utils";

/** Side of the white QR plate, in pixels. Fixed so hydration shifts no layout. */
const QR_PLATE_PX = 132;

interface EncodedQr {
  /** The URL this symbol encodes, used to reject a symbol from a previous code. */
  url: string;
  path: string;
  extent: number;
}

/**
 * Encodes `url` into QR path data AFTER a paint boundary (rule 24).
 *
 * The card must appear the instant a code is minted, and the manual code is the
 * primary path — neither may wait on encoding. A version-5 symbol costs about a
 * millisecond, which is small but not nothing, and it has no business running
 * inside the click commit. Returning null on the first render keeps it out.
 *
 * The stored URL is compared against the current one before the symbol is shown,
 * so the frame after a regeneration can never display the previous code's QR. A
 * stale symbol here would point a phone at a code that is already burned.
 */
function useDeferredQr(url: string | null): EncodedQr | null {
  const [encoded, setEncoded] = useState<EncodedQr | null>(null);

  useEffect(() => {
    if (!url) {
      setEncoded(null);
      return undefined;
    }

    const job = scheduleAfterPaint(() => {
      const matrix = encodeQrMatrix(url);
      setEncoded({
        url,
        path: qrMatrixToPath(matrix),
        extent: qrSvgExtent(matrix),
      });
    });

    return () => cancelScheduledJob(job);
  }, [url]);

  return encoded && encoded.url === url ? encoded : null;
}

/**
 * The scannable symbol.
 *
 * Colours are literal `#000000` on `#FFFFFF` in both themes: a scanner expects
 * that polarity, and per the WKWebView rules a dropped `var()` here would not
 * look wrong so much as silently fail to scan. The plate keeps its size before
 * the symbol arrives so hydration never nudges the layout.
 */
function PairingQrCode({ url }: { url: string }) {
  const qr = useDeferredQr(url);

  return (
    <div className="shrink-0 space-y-1.5">
      <div
        data-testid="remote-pairing-qr"
        role="img"
        aria-label="Scan to pair this device"
        className="rounded-md"
        style={{
          backgroundColor: "#FFFFFF",
          borderColor: "var(--border-default, #393940)",
          borderStyle: "solid",
          borderWidth: "1px",
          width: `${QR_PLATE_PX}px`,
          height: `${QR_PLATE_PX}px`,
        }}
      >
        {qr && (
          <svg
            viewBox={`0 0 ${qr.extent} ${qr.extent}`}
            width="100%"
            height="100%"
            shapeRendering="crispEdges"
            aria-hidden="true"
          >
            <rect width={qr.extent} height={qr.extent} fill="#FFFFFF" />
            <path d={qr.path} fill="#000000" />
          </svg>
        )}
      </div>
      <p className="text-[10px] text-center text-[var(--text-muted)]">
        Scan on the device
      </p>
    </div>
  );
}

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

/**
 * Ticks once a second while some code is on screen; freezes when there is none.
 *
 * Keyed on the identity of whatever is being counted down rather than on a boolean, so
 * swapping one code for another restarts the clock while a steady code keeps its interval.
 * Every countdown is derived from the backend's `expiresAt`, never from an elapsed-time
 * counter, so remounting the pane resumes the real remaining time instead of restarting.
 */
function useTickingNow(tickKey: string | null): number {
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    if (tickKey === null) {
      return;
    }
    setNowMs(Date.now());
    const interval = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(interval);
  }, [tickKey]);
  return nowMs;
}

/** The outstanding code that lapses first — the one worth counting down. */
function soonestOutstanding(
  codes: RemotePairingCodeView[] | null,
): RemotePairingCodeView | null {
  if (!codes || codes.length === 0) {
    return null;
  }
  return codes.reduce((earliest, code) =>
    Date.parse(code.expiresAt) < Date.parse(earliest.expiresAt) ? code : earliest,
  );
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
  // The soonest-lapsing outstanding code is picked WITHOUT consulting the clock, so it can key
  // the ticker without the ticker feeding back into its own selection.
  const soonest = soonestOutstanding(outstandingCodes);
  const tickKey = pairing
    ? `minted:${pairing.id}`
    : soonest
      ? `outstanding:${soonest.id}`
      : null;
  const nowMs = useTickingNow(tickKey);

  const remaining = pairing ? remainingSeconds(pairing.expiresAt, nowMs) : 0;
  const expired = pairing !== null && remaining <= 0;

  /**
   * A code minted before this pane last unmounted is still live and still redeemable, so the
   * card has to say so. It deliberately does NOT try to redisplay the code: the backend stores
   * only `code_hash` (`entities/remote_access.rs`) and `list_remote_pairing_codes` returns
   * metadata with no `code` field, exactly like the device token. Weakening that to make the
   * card prettier would turn a write-once secret into a readable one, so the honest state is
   * "one is active, here is how long it has left, cancel it or mint another".
   */
  const activeRemaining = soonest ? remainingSeconds(soonest.expiresAt, nowMs) : 0;
  const restored = !pairing && soonest !== null && activeRemaining > 0 ? soonest : null;

  // Fire onExpired exactly once per code as its countdown crosses zero.
  const notifiedExpiryFor = useRef<string | null>(null);
  useEffect(() => {
    if (expired && pairing && notifiedExpiryFor.current !== pairing.id) {
      notifiedExpiryFor.current = pairing.id;
      onExpired();
    }
  }, [expired, pairing, onExpired]);

  // Same, for a restored code lapsing on screen: re-read the backend rather than assume the
  // local clock's verdict is the durable one.
  const notifiedOutstandingExpiryFor = useRef<string | null>(null);
  useEffect(() => {
    if (
      soonest &&
      !pairing &&
      activeRemaining <= 0 &&
      notifiedOutstandingExpiryFor.current !== soonest.id
    ) {
      notifiedOutstandingExpiryFor.current = soonest.id;
      onExpired();
    }
  }, [soonest, pairing, activeRemaining, onExpired]);

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
                : restored
                  ? "Generating another code leaves the active one valid — cancel it above if you no longer want it."
                  : "Generate a code, then scan or paste it on the device you are pairing."}
            </p>
            <Button
              data-testid="remote-pair-device"
              size="sm"
              onClick={onGenerate}
              disabled={pairingBusy || !listenerEnabled}
              className="bg-[var(--accent-primary)] hover:bg-[var(--accent-hover)] text-[var(--text-on-accent)] shrink-0"
            >
              {pairingBusy
                ? "Generating…"
                : restored
                  ? "Generate a new code"
                  : "Generate pairing code"}
            </Button>
          </div>
        ) : null}

        {/* A code that outlived the pane's last unmount. Honest by construction: the raw code
            is unrecoverable, so this states that one is outstanding and how long it has left
            rather than implying it can be read back. */}
        {restored && (
          <div
            data-testid="remote-pairing-active"
            className="rounded-md p-4 space-y-2"
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
                testId="remote-pairing-active-countdown"
                label={`Expires in ${formatCountdown(activeRemaining)}`}
              />
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    data-testid="remote-pairing-active-cancel"
                    aria-label="Cancel the active pairing code"
                    onClick={() => onCancel(restored.id)}
                  >
                    <X className="h-4 w-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>Cancel the active pairing code</TooltipContent>
              </Tooltip>
            </div>
            <p className="text-sm font-medium text-[var(--text-primary)]">
              A pairing code is still active
            </p>
            <p className="text-xs text-[var(--text-muted)]">
              It was created {formatDateTime(restored.createdAt)} and can still be
              redeemed. Codes are shown only once, when they are generated, so this one
              cannot be displayed again — cancel it, or generate a new code to pair with.
            </p>
          </div>
        )}
        {!listenerEnabled && !pairing && (
          <p className="text-xs text-[var(--text-muted)]">
            Enable remote access first — a device cannot redeem a code while the
            listener is off.
          </p>
        )}
        {expired && (
          <NoticeBanner tone="warning" testId="remote-pairing-expired">
            Pairing code expired. Expired codes are single-use and can no longer
            be redeemed.
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

            {/* Manual entry stays the primary path (R-12); the QR is the shortcut
                beside it, and wraps underneath once the pane gets narrow. */}
            <div className="flex flex-wrap items-start gap-4">
              <div className="space-y-1 min-w-[11rem] flex-1">
                <p className="text-xs text-[var(--text-muted)]">
                  Enter this code on the device
                </p>
                <p
                  data-testid="remote-pairing-code"
                  className="font-mono text-lg tracking-wide text-[var(--text-primary)] select-all"
                >
                  <span className="text-[var(--text-muted)]">
                    {grouped.prefix}
                  </span>
                  {grouped.groups.map((group, index) => (
                    <span key={index}>
                      {index > 0 && (
                        <span className="text-[var(--text-muted)]"> </span>
                      )}
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

              {/* No advertised endpoint means no URL, so there is nothing to
                  encode — R-12 forbids inventing a multi-endpoint payload. */}
              {pairingUrl && <PairingQrCode url={pairingUrl} />}
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
                  No advertised endpoint is known yet — enter the host address
                  and the code manually on the device.
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
                    <TooltipContent>
                      Cancel outstanding pairing code
                    </TooltipContent>
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
