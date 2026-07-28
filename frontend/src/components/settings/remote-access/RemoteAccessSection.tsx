/**
 * RemoteAccessSection — Settings → Remote Access pane (PR 1.7, §5.4).
 *
 * Rule 24: the pane paints its full shell synchronously; every backend invoke is
 * deferred behind a paint boundary (scheduleAfterPaint). Toggles update visible
 * state optimistically and revert on error.
 *
 * Feature-gated on `remoteEnvironments` (`ui.feature_flags`, ui_commands.rs) —
 * renders nothing while the flag is off.
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { RadioTower } from "lucide-react";

import {
  REMOTE_SESSION_CLOSED_EVENT,
  REMOTE_SESSION_CONNECTED_EVENT,
  remoteHostApi,
  type AdvertisedEndpoint,
  type MintedRemotePairingCode,
  type RemoteAuditEntry,
  type RemoteDeviceView,
  type RemoteExposureMode,
  type RemoteListenerStatus,
  type RemotePairingCodeView,
  type RemoteSessionView,
} from "@/api/remote-host";
import { Card } from "@/components/ui/card";
import { CopyableRef } from "@/components/ui/copyable-ref";
import { NoticeBanner } from "@/components/ui/notice-banner";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import { useEventBus } from "@/providers/EventProvider";

import {
  cancelScheduledJob,
  scheduleAfterPaint,
} from "../SettingsDialog.performance";

import { RemoteDeviceList } from "./RemoteDeviceList";
import { RemotePairingCard } from "./RemotePairingCard";
import { RemoteSessionList } from "./RemoteSessionList";
import { pickPreferredEndpoint } from "./remote-access-utils";

// ============================================================================
// Shared building blocks
// ============================================================================

export function RemoteAccessCardHeader({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <>
      <div className="flex items-start gap-3 p-5 pb-0">
        <div className="p-2 rounded-lg bg-[var(--accent-muted)] shrink-0">
          <RadioTower className="w-[18px] h-[18px] text-[var(--card-icon-color)]" />
        </div>
        <div>
          <h3 className="text-sm font-semibold tracking-tight text-[var(--text-primary)]">
            {title}
          </h3>
          <p className="text-xs text-[var(--text-muted)] mt-0.5">{description}</p>
        </div>
      </div>
      <Separator className="my-4 bg-[var(--border-subtle)]" />
    </>
  );
}

export function RemoteAccessSkeletonRows({ rows = 2 }: { rows?: number }) {
  return (
    <div className="space-y-2" aria-hidden="true">
      {Array.from({ length: rows }, (_, index) => (
        <div
          key={index}
          className="h-10 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]"
        />
      ))}
    </div>
  );
}

function errorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) {
    return error.message;
  }
  return typeof error === "string" && error.length > 0 ? error : fallback;
}

/** Paint-boundary gate (rule 24): true only after a frame + macrotask have passed. */
function usePaintBoundaryHydration(): boolean {
  const [hydrated, setHydrated] = useState(false);
  useEffect(() => {
    const job = scheduleAfterPaint(() => setHydrated(true));
    return () => cancelScheduledJob(job);
  }, []);
  return hydrated;
}

// ============================================================================
// Listener card (enable toggle + exposure mode + endpoints)
// ============================================================================

const EXPOSURE_MODES: { mode: RemoteExposureMode; label: string; hint: string }[] = [
  {
    mode: "serve",
    label: "Tailscale Serve",
    hint: "TLS at the tailnet edge via your MagicDNS name",
  },
  {
    mode: "tailnetDirect",
    label: "Tailnet direct",
    hint: "Plain HTTP on your tailnet IP, inside WireGuard",
  },
];

const ENDPOINT_KIND_LABELS: Record<AdvertisedEndpoint["kind"], string> = {
  loopbackServe: "Tailscale Serve",
  tailnetDirect: "Tailnet direct",
};

interface ListenerCardProps {
  status: RemoteListenerStatus | null;
  displayEnabled: boolean;
  displayMode: RemoteExposureMode;
  busy: boolean;
  endpoints: AdvertisedEndpoint[] | null;
  endpointsUnavailable: boolean;
  onEnabledChange: (enabled: boolean) => void;
  onModeChange: (mode: RemoteExposureMode) => void;
}

function ListenerCard({
  status,
  displayEnabled,
  displayMode,
  busy,
  endpoints,
  endpointsUnavailable,
  onEnabledChange,
  onModeChange,
}: ListenerCardProps) {
  const serveDegraded =
    status !== null &&
    status.enabled &&
    displayMode === "serve" &&
    !status.serveActive;

  return (
    <Card className="bg-[var(--bg-elevated)] border-[var(--border-default)] shadow-[var(--shadow-xs)]">
      <RemoteAccessCardHeader
        title="Remote Access"
        description="Let your own paired devices reach this Mac over your tailnet"
      />
      <div className="px-5 pb-5 space-y-4">
        <div className="flex items-start justify-between">
          <div className="flex-1 min-w-0 pr-4">
            <p className="text-sm font-medium text-[var(--text-primary)]">
              Enable remote access
            </p>
            <p className="text-xs text-[var(--text-muted)] mt-0.5">
              {status?.running && status.bindAddress
                ? `Listening on ${status.bindAddress}`
                : "Starts the remote listener for paired devices"}
            </p>
          </div>
          <Switch
            data-testid="remote-enable-toggle"
            checked={displayEnabled}
            onCheckedChange={onEnabledChange}
            disabled={busy || status === null}
            aria-label="Enable remote access"
            className="data-[state=checked]:bg-[var(--accent-primary)]"
          />
        </div>

        <div>
          <p className="text-xs font-medium text-[var(--text-secondary)] mb-2">
            Exposure mode
          </p>
          <div
            role="radiogroup"
            aria-label="Exposure mode"
            className="grid grid-cols-1 sm:grid-cols-2 gap-2"
          >
            {EXPOSURE_MODES.map(({ mode, label, hint }) => {
              const selected = displayMode === mode;
              return (
                <button
                  key={mode}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  data-testid={`remote-mode-${mode}`}
                  disabled={busy || status === null}
                  onClick={() => {
                    if (!selected) {
                      onModeChange(mode);
                    }
                  }}
                  className="rounded-md px-3 py-2 text-left transition-colors disabled:opacity-50"
                  style={{
                    backgroundColor: selected
                      ? "var(--accent-muted, rgba(255, 107, 53, 0.12))"
                      : "var(--bg-surface, #1e1e23)",
                    borderColor: selected
                      ? "var(--accent-border, #ff6b35)"
                      : "var(--border-default, #393940)",
                    borderStyle: "solid",
                    borderWidth: "1px",
                  }}
                >
                  <span className="block text-sm font-medium text-[var(--text-primary)]">
                    {label}
                  </span>
                  <span className="block text-xs text-[var(--text-muted)] mt-0.5">
                    {hint}
                  </span>
                </button>
              );
            })}
          </div>
        </div>

        {serveDegraded && (
          <NoticeBanner tone="warning" testId="remote-serve-degraded">
            Tailscale Serve is not active
            {status.serveDegradedReason ? `: ${status.serveDegradedReason}` : "."}
          </NoticeBanner>
        )}

        <div>
          <p className="text-xs font-medium text-[var(--text-secondary)] mb-2">
            Advertised endpoints
          </p>
          {endpointsUnavailable ? (
            <NoticeBanner tone="neutral" testId="remote-endpoints-unavailable">
              Endpoint discovery is not available yet — the advertised-endpoint
              backend surface (PR 1.6) has not landed. Pair using the listener
              address below or enter the host manually on the client.
            </NoticeBanner>
          ) : endpoints === null ? (
            <RemoteAccessSkeletonRows rows={1} />
          ) : endpoints.length === 0 ? (
            <p className="text-xs text-[var(--text-muted)]">
              No endpoints advertised — check that Tailscale is running and logged
              in.
            </p>
          ) : (
            <ul data-testid="remote-endpoints" className="space-y-1.5">
              {endpoints.map((endpoint) => (
                <li key={endpoint.url} className="flex items-center gap-2 text-sm">
                  <span
                    data-testid={`remote-endpoint-dot-${endpoint.available ? "up" : "down"}`}
                    aria-label={endpoint.available ? "Reachable" : "Unreachable"}
                    className="inline-block h-2 w-2 rounded-full shrink-0"
                    style={{
                      backgroundColor: endpoint.available
                        ? "var(--status-success, #2eb867)"
                        : "var(--status-error, #d55e00)",
                    }}
                  />
                  <CopyableRef
                    value={endpoint.url}
                    ariaLabel="Copy endpoint URL"
                    testId={`remote-endpoint-${endpoint.kind}`}
                  />
                  <span className="text-xs text-[var(--text-muted)] shrink-0">
                    {ENDPOINT_KIND_LABELS[endpoint.kind]}
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

// ============================================================================
// RemoteAccessSection
// ============================================================================

function RemoteAccessPanel() {
  const bus = useEventBus();
  const hydrated = usePaintBoundaryHydration();

  const [status, setStatus] = useState<RemoteListenerStatus | null>(null);
  const [pendingEnabled, setPendingEnabled] = useState<boolean | null>(null);
  const [pendingMode, setPendingMode] = useState<RemoteExposureMode | null>(null);
  const [listenerBusy, setListenerBusy] = useState(false);
  const [endpoints, setEndpoints] = useState<AdvertisedEndpoint[] | null>(null);
  const [endpointsUnavailable, setEndpointsUnavailable] = useState(false);
  const [devices, setDevices] = useState<RemoteDeviceView[] | null>(null);
  const [sessions, setSessions] = useState<RemoteSessionView[] | null>(null);
  const [outstandingCodes, setOutstandingCodes] = useState<
    RemotePairingCodeView[] | null
  >(null);
  const [auditEntries, setAuditEntries] = useState<RemoteAuditEntry[] | null>(null);
  const [auditUnavailable, setAuditUnavailable] = useState(false);
  const [pairing, setPairing] = useState<MintedRemotePairingCode | null>(null);
  const [pairingBusy, setPairingBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const loadStatus = useCallback(async () => {
    try {
      setStatus(await remoteHostApi.getListenerStatus());
    } catch (error) {
      setActionError(errorMessage(error, "Failed to load remote access status"));
    }
  }, []);

  const loadEndpoints = useCallback(async () => {
    try {
      setEndpoints(await remoteHostApi.listAdvertisedEndpoints());
      setEndpointsUnavailable(false);
    } catch {
      // Known gap: the PR 1.6 advertised-endpoint command is not registered yet.
      // Fail visible (explicit degraded note), never silent-empty.
      setEndpointsUnavailable(true);
    }
  }, []);

  const loadDevices = useCallback(async () => {
    try {
      setDevices(await remoteHostApi.listDevices());
    } catch (error) {
      setActionError(errorMessage(error, "Failed to load paired devices"));
    }
  }, []);

  const loadSessions = useCallback(async () => {
    try {
      setSessions(await remoteHostApi.listSessions());
    } catch (error) {
      setActionError(errorMessage(error, "Failed to load live sessions"));
    }
  }, []);

  const loadOutstandingCodes = useCallback(async () => {
    try {
      setOutstandingCodes(await remoteHostApi.listPairingCodes());
    } catch (error) {
      setActionError(errorMessage(error, "Failed to load outstanding pairing codes"));
    }
  }, []);

  const loadAudit = useCallback(async () => {
    try {
      setAuditEntries(await remoteHostApi.listAuditEntries());
      setAuditUnavailable(false);
    } catch {
      // Known gap: no audit-list command is registered yet (PR 1.2 follow-up).
      setAuditUnavailable(true);
    }
  }, []);

  // Hydrate only after the paint boundary (rule 24).
  useEffect(() => {
    if (!hydrated) {
      return;
    }
    void loadStatus();
    void loadEndpoints();
    void loadDevices();
    void loadSessions();
    void loadOutstandingCodes();
    void loadAudit();
  }, [
    hydrated,
    loadStatus,
    loadEndpoints,
    loadDevices,
    loadSessions,
    loadOutstandingCodes,
    loadAudit,
  ]);

  // Local-only session lifecycle events keep the lists live (§5.5).
  useEffect(() => {
    if (!hydrated) {
      return;
    }
    const refresh = () => {
      void loadSessions();
      void loadDevices();
    };
    const unsubscribeConnected = bus.subscribe(REMOTE_SESSION_CONNECTED_EVENT, refresh);
    const unsubscribeClosed = bus.subscribe(REMOTE_SESSION_CLOSED_EVENT, refresh);
    return () => {
      unsubscribeConnected();
      unsubscribeClosed();
    };
  }, [bus, hydrated, loadSessions, loadDevices]);

  const displayEnabled = pendingEnabled ?? status?.enabled ?? false;
  const displayMode = pendingMode ?? status?.exposureMode ?? "serve";

  const handleEnabledChange = useCallback(
    (enabled: boolean) => {
      // Paint first: the switch reflects the new state before the invoke settles.
      setPendingEnabled(enabled);
      setListenerBusy(true);
      setActionError(null);
      void (enabled ? remoteHostApi.startListener() : remoteHostApi.stopListener())
        .then((next) => {
          setStatus(next);
          void loadEndpoints();
          if (!enabled) {
            // Disable tears down every live session immediately (§4.4).
            void loadSessions();
          }
        })
        .catch((error: unknown) => {
          setActionError(
            errorMessage(
              error,
              enabled ? "Failed to start remote access" : "Failed to stop remote access",
            ),
          );
        })
        .finally(() => {
          setPendingEnabled(null);
          setListenerBusy(false);
        });
    },
    [loadEndpoints, loadSessions],
  );

  const handleModeChange = useCallback(
    (mode: RemoteExposureMode) => {
      setPendingMode(mode);
      setListenerBusy(true);
      setActionError(null);
      void remoteHostApi
        .setExposureMode(mode)
        .then((next) => {
          setStatus(next);
          void loadEndpoints();
        })
        .catch((error: unknown) => {
          setActionError(errorMessage(error, "Failed to change exposure mode"));
        })
        .finally(() => {
          setPendingMode(null);
          setListenerBusy(false);
        });
    },
    [loadEndpoints],
  );

  const handleGeneratePairingCode = useCallback(() => {
    setPairingBusy(true);
    setActionError(null);
    void remoteHostApi
      .generatePairingCode()
      .then((minted) => {
        setPairing(minted);
        void loadOutstandingCodes();
      })
      .catch((error: unknown) => {
        setActionError(errorMessage(error, "Failed to generate a pairing code"));
      })
      .finally(() => {
        setPairingBusy(false);
      });
  }, [loadOutstandingCodes]);

  const handleCancelPairingCode = useCallback(
    (id: string) => {
      // Immediate visual dismissal; the backend cancel follows.
      setPairing((current) => (current?.id === id ? null : current));
      setOutstandingCodes(
        (current) => current?.filter((code) => code.id !== id) ?? current,
      );
      void remoteHostApi
        .revokePairingCode(id)
        .catch((error: unknown) => {
          setActionError(errorMessage(error, "Failed to cancel the pairing code"));
        })
        .finally(() => {
          void loadOutstandingCodes();
        });
    },
    [loadOutstandingCodes],
  );

  const handlePairingExpired = useCallback(() => {
    void loadOutstandingCodes();
  }, [loadOutstandingCodes]);

  const handleSetAgentControl = useCallback(
    (deviceId: string, enabled: boolean) => {
      // Optimistic: the toggle reflects the decision immediately; revert on error.
      setDevices(
        (current) =>
          current?.map((device) =>
            device.id === deviceId
              ? { ...device, agentControlGranted: enabled }
              : device,
          ) ?? current,
      );
      setActionError(null);
      void remoteHostApi
        .setDeviceAgentControl(deviceId, enabled)
        .then((view) => {
          setDevices(
            (current) =>
              current?.map((device) => (device.id === view.id ? view : device)) ??
              current,
          );
          if (!enabled) {
            // Withdrawal fires the device's kill channels — session list changes now.
            void loadSessions();
          }
        })
        .catch((error: unknown) => {
          setDevices(
            (current) =>
              current?.map((device) =>
                device.id === deviceId
                  ? { ...device, agentControlGranted: !enabled }
                  : device,
              ) ?? current,
          );
          setActionError(errorMessage(error, "Failed to update remote agent control"));
        });
    },
    [loadSessions],
  );

  const handleRevokeDevice = useCallback(
    (deviceId: string) => {
      setActionError(null);
      void remoteHostApi
        .revokeDevice(deviceId)
        .then((view) => {
          setDevices(
            (current) =>
              current?.map((device) => (device.id === view.id ? view : device)) ??
              current,
          );
          // Revocation tears down the device's live sessions immediately (§4.4).
          void loadSessions();
        })
        .catch((error: unknown) => {
          setActionError(errorMessage(error, "Failed to revoke the device"));
        });
    },
    [loadSessions],
  );

  const handleDisconnectSession = useCallback(
    (sessionId: string) => {
      // Immediate: the row leaves the list before the invoke settles.
      setSessions(
        (current) => current?.filter((session) => session.id !== sessionId) ?? current,
      );
      setActionError(null);
      void remoteHostApi
        .disconnectSession(sessionId)
        .catch((error: unknown) => {
          setActionError(errorMessage(error, "Failed to disconnect the session"));
        })
        .finally(() => {
          void loadSessions();
        });
    },
    [loadSessions],
  );

  const preferredEndpoint = useMemo(
    () => (status ? pickPreferredEndpoint(endpoints, status) : null),
    [endpoints, status],
  );

  return (
    <div data-testid="remote-access-section" className="space-y-4">
      {actionError && (
        <NoticeBanner tone="error" testId="remote-access-error">
          {actionError}
        </NoticeBanner>
      )}

      <ListenerCard
        status={status}
        displayEnabled={displayEnabled}
        displayMode={displayMode}
        busy={listenerBusy}
        endpoints={endpoints}
        endpointsUnavailable={endpointsUnavailable}
        onEnabledChange={handleEnabledChange}
        onModeChange={handleModeChange}
      />

      <RemotePairingCard
        pairing={pairing}
        pairingBusy={pairingBusy}
        listenerEnabled={displayEnabled}
        preferredEndpoint={preferredEndpoint}
        outstandingCodes={outstandingCodes}
        onGenerate={handleGeneratePairingCode}
        onCancel={handleCancelPairingCode}
        onExpired={handlePairingExpired}
      />

      <RemoteDeviceList
        devices={devices}
        onSetAgentControl={handleSetAgentControl}
        onRevoke={handleRevokeDevice}
      />

      <RemoteSessionList
        devices={devices}
        sessions={sessions}
        auditEntries={auditEntries}
        auditUnavailable={auditUnavailable}
        onDisconnect={handleDisconnectSession}
      />
    </div>
  );
}

export function RemoteAccessSection() {
  const { data: flags } = useFeatureFlags();
  // Inert while the flag is off (§8 flags note): no shell, no invokes, no listeners.
  if (!flags.remoteEnvironments) {
    return null;
  }
  return <RemoteAccessPanel />;
}
