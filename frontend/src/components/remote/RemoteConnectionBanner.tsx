/**
 * The degraded-mode banner for the ACTIVE remote environment (PR 2.7-a).
 *
 * A pure READER of `environmentStore.connectionPresentations`, which the composition
 * root in `lib/remote/environment-runtime.ts` is the single writer of. Nothing here
 * subscribes to a supervisor, and — A-5 — nothing here schedules a retry: `Try again`
 * is a user-driven wakeup routed through the runtime's `retryNow` action, the only
 * non-timer entry into the FSM's blocked state.
 *
 * Renders NOTHING for: the local environment, a `connected` remote, a `suspended`
 * remote (backgrounded is not an error), and a disabled `remoteEnvironments` flag.
 * `connecting` (first ever connect) gets a slim variant that makes no read-only claim,
 * because there is no cached data to be read-only about yet.
 */

import { useState } from "react";

import { AlertTriangle, RefreshCw, WifiOff } from "lucide-react";

import { Button } from "@/components/ui/button";
import { NoticeBanner } from "@/components/ui/notice-banner";
import { retryActiveEnvironmentNow } from "@/lib/remote/environment-runtime";
import { LOCAL_ENVIRONMENT_ID } from "@/lib/remote/active-environment";
import { useEnvironmentStore } from "@/stores/environmentStore";
import { useUiStore } from "@/stores/uiStore";

import { RemoteConnectionJournalDialog } from "./RemoteConnectionJournalDialog";

function blockedCopy(
  failure: string | null,
  name: string,
  message: string | null
): { title: string; body: string; showRetry: boolean; showRePair: boolean } {
  switch (failure) {
    case "version":
      return {
        title: `"${name}" needs a newer client`,
        body:
          message ??
          "This host requires a newer protocol than this app speaks. Update RalphX, then try again.",
        showRetry: true,
        showRePair: false,
      };
    case "unauthorized":
      return {
        title: `Access to "${name}" was revoked`,
        body:
          "The host ended this device's session. Re-pair the environment to reconnect.",
        showRetry: false,
        showRePair: true,
      };
    case "invalid_request":
      return {
        title: `"${name}" rejected this app's request`,
        body:
          message ??
          "The host understood the connection but rejected the request this app sent. Update RalphX, then try again.",
        showRetry: true,
        showRePair: false,
      };
    case "malformed_descriptor":
      return {
        title: `"${name}" sent an invalid identity response`,
        body:
          message ??
          "The host's identity response could not be understood, so this client will not authenticate against it.",
        showRetry: true,
        showRePair: false,
      };
    default:
      // Fail closed: an unknown blocked cause is still blocked, and the honest offer is
      // the one action that cannot make it worse.
      return {
        title: `"${name}" can't be reached right now`,
        body: message ?? "The connection is blocked and will not retry on its own.",
        showRetry: true,
        showRePair: false,
      };
  }
}

export function RemoteConnectionBanner() {
  const enabled = useUiStore((state) => state.featureFlags.remoteEnvironments);
  const openModal = useUiStore((state) => state.openModal);
  const [journalOpen, setJournalOpen] = useState(false);
  const activeEnvironmentId = useEnvironmentStore(
    (state) => state.activeEnvironmentId
  );
  const isRemote = useEnvironmentStore((state) => {
    if (state.activeEnvironmentId === LOCAL_ENVIRONMENT_ID) return false;
    const entry = state.environments.find(
      (candidate) => candidate.id === state.activeEnvironmentId
    );
    // An id with no entry yet is treated as remote: fail closed, since the only way to
    // be sure an environment is local is to find a local row for it.
    return entry === undefined || entry.kind === "remote";
  });
  const name = useEnvironmentStore(
    (state) =>
      state.environments.find(
        (candidate) => candidate.id === state.activeEnvironmentId
      )?.name ?? state.activeEnvironmentId
  );
  const presentation = useEnvironmentStore(
    (state) =>
      state.connectionPresentations[state.activeEnvironmentId]?.presentation
  );
  const blockedFailure = useEnvironmentStore(
    (state) =>
      state.connectionPresentations[state.activeEnvironmentId]?.blockedFailure ??
      null
  );
  const blockedMessage = useEnvironmentStore(
    (state) =>
      state.connectionPresentations[state.activeEnvironmentId]?.blockedMessage ??
      null
  );

  if (
    enabled !== true ||
    !isRemote ||
    presentation === undefined ||
    presentation === "connected" ||
    presentation === "suspended" ||
    // `syncing` is chip-only chrome: the transport is provably live and the client is
    // reading data, so a full-width banner (and its app-body reflow) would present a
    // routine hydration as an outage. The environment switcher shows the pulse; the
    // supervisor escalates to `reconnecting` if the sync misbehaves.
    presentation === "syncing"
  ) {
    return null;
  }

  // Every degraded presentation carries the same escape hatch: the connection log.
  // "Reconnecting…" without a WHY is a dead end; the journal names the failing step.
  const detailsButton = (
    <Button
      type="button"
      variant="outline"
      size="sm"
      className="h-7 px-2 text-xs"
      data-testid="remote-connection-banner-details"
      onClick={() => setJournalOpen(true)}
    >
      Details
    </Button>
  );
  const journalDialog = (
    <RemoteConnectionJournalDialog
      environmentId={activeEnvironmentId}
      environmentName={name}
      open={journalOpen}
      onOpenChange={setJournalOpen}
    />
  );

  if (presentation === "connecting") {
    return (
      <>
        <BannerFrame
          tone="neutral"
          icon={<RefreshCw size={14} aria-hidden="true" />}
          title={`Connecting to "${name}"…`}
          testId="remote-connection-banner"
          presentation={presentation}
          action={detailsButton}
        >
          Setting up this environment for the first time.
        </BannerFrame>
        {journalDialog}
      </>
    );
  }

  if (presentation === "reconnecting") {
    return (
      <>
        <BannerFrame
          tone="warning"
          icon={<RefreshCw size={14} aria-hidden="true" />}
          title={`Reconnecting to "${name}"…`}
          testId="remote-connection-banner"
          presentation={presentation}
          action={detailsButton}
        >
          Viewing cached data (read-only until the connection returns).
        </BannerFrame>
        {journalDialog}
      </>
    );
  }

  if (presentation === "offline") {
    return (
      <>
        <BannerFrame
          tone="neutral"
          icon={<WifiOff size={14} aria-hidden="true" />}
          title="You're offline"
          testId="remote-connection-banner"
          presentation={presentation}
          action={detailsButton}
        >
          {`"${name}" will reconnect when the network returns. Cached data shown read-only.`}
        </BannerFrame>
        {journalDialog}
      </>
    );
  }

  const copy = blockedCopy(blockedFailure, name, blockedMessage);
  return (
    <>
      <BannerFrame
        tone="error"
        icon={<AlertTriangle size={14} aria-hidden="true" />}
        title={copy.title}
        testId="remote-connection-banner"
        presentation={presentation}
        blockedFailure={blockedFailure}
        action={
          <span className="flex shrink-0 items-center gap-1.5">
            {detailsButton}
            {copy.showRetry ? (
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-7 px-2 text-xs"
                data-testid="remote-connection-banner-retry"
                onClick={() => retryActiveEnvironmentNow(activeEnvironmentId)}
              >
                Try again
              </Button>
            ) : null}
            {copy.showRePair ? (
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-7 px-2 text-xs"
                data-testid="remote-connection-banner-repair"
                onClick={() => openModal("settings", { section: "connections" })}
              >
                Re-pair…
              </Button>
            ) : null}
          </span>
        }
      >
        {copy.body}
      </BannerFrame>
      {journalDialog}
    </>
  );
}

function BannerFrame({
  tone,
  icon,
  title,
  children,
  action,
  testId,
  presentation,
  blockedFailure,
}: {
  tone: "warning" | "error" | "neutral";
  icon: React.ReactNode;
  title: string;
  children: React.ReactNode;
  action?: React.ReactNode;
  testId: string;
  presentation: string;
  blockedFailure?: string | null;
}) {
  return (
    <div
      className="px-3 pt-2"
      data-testid={`${testId}-host`}
      data-presentation={presentation}
      {...(blockedFailure ? { "data-blocked-failure": blockedFailure } : {})}
    >
      <NoticeBanner
        tone={tone}
        icon={icon}
        title={title}
        testId={testId}
        className="py-1.5"
        {...(action === undefined ? {} : { action })}
      >
        {children}
      </NoticeBanner>
    </div>
  );
}
