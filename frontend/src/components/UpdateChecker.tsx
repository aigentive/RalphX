import { useCallback, useEffect, useRef, useState } from "react";
import { check } from "@tauri-apps/plugin-updater";
import { toast } from "sonner";
import {
  getCurrentReleaseNotes,
  getLastSeenReleaseNotesVersion,
} from "@/api/release-notes";
import type { UpdateChannel } from "@/api/update-channel";
import { ReleaseNotesDialog } from "@/components/ReleaseNotesDialog";
import { useUpdateChannel } from "@/hooks/useUpdateChannel";
import { useUiStore } from "@/stores/uiStore";
import { installUpdate } from "./UpdateChecker.install";
import { useUpdateCheckerNativeEvents } from "./UpdateChecker.events";
import {
  sanitizeReleaseNotesBody,
  showUpdateNotification,
  showWhatsNewToast,
  updateChannelLabel,
  whatsNewToastId,
  type ReleaseNotesView,
} from "./UpdateChecker.toasts";

const INITIAL_UPDATE_CHECK_DELAY_MS = 3_000;
const STARTUP_RELEASE_NOTES_DELAY_MS = 4_000;
const UPDATE_CHECK_INTERVAL_MS = 30 * 60 * 1_000;
const LIFECYCLE_UPDATE_CHECK_COOLDOWN_MS = 5 * 60 * 1_000;
const UPDATE_CHECK_RESULT_TOAST_ID = "update-check-result";
interface ReleaseDialogState {
  open: boolean;
  version?: string | undefined;
  body?: string | null | undefined;
  context?: "current" | "update" | undefined;
  channel?: UpdateChannel | undefined;
}

interface CheckForUpdatesOptions {
  manual?: boolean;
  force?: boolean;
  target?: UpdateChannel;
}

type CheckForUpdates = (options?: CheckForUpdatesOptions) => Promise<void>;

interface UpdateCheckerProps {
  automaticMaintenanceEnabled?: boolean;
  checkForUpdatesRequest?: number;
  listenForNativeActions?: boolean;
  openReleaseNotesRequest?: number;
}

export function UpdateChecker({
  automaticMaintenanceEnabled = true,
  checkForUpdatesRequest = 0,
  listenForNativeActions = true,
  openReleaseNotesRequest = 0,
}: UpdateCheckerProps = {}) {
  const activeModal = useUiStore((s) => s.activeModal);
  const {
    updateChannel,
    isSettled: isUpdateChannelSettled,
    isError: isUpdateChannelError,
    loadError: updateChannelError,
  } = useUpdateChannel();
  const checkInFlight = useRef(false);
  const notifiedVersion = useRef<string | null>(null);
  const lastCheckAt = useRef<number | null>(null);
  const activeUpdateChannel = useRef<UpdateChannel | null>(null);
  const checkGeneration = useRef(0);
  const queuedForcedCheck = useRef<CheckForUpdatesOptions | null>(null);
  const queuedUnsettledCheck = useRef<CheckForUpdatesOptions | null>(null);
  const checkForUpdatesRef = useRef<CheckForUpdates | null>(null);
  const whatsNewVersion = useRef<string | null>(null);
  const pendingWhatsNew = useRef<ReleaseNotesView | null>(null);
  const visibleWhatsNew = useRef<ReleaseNotesView | null>(null);
  const visibleWhatsNewToastId = useRef<string | null>(null);
  const isGlobalModalOpen = useRef(activeModal !== null);
  const handledCheckForUpdatesRequest = useRef(0);
  const handledOpenReleaseNotesRequest = useRef(0);
  const [dialogState, setDialogState] = useState<ReleaseDialogState>({ open: false });

  const clearVisibleWhatsNew = useCallback((version?: string) => {
    if (version !== undefined && visibleWhatsNew.current?.version !== version) {
      return;
    }
    visibleWhatsNew.current = null;
    visibleWhatsNewToastId.current = null;
  }, []);

  const openReleaseNotes = useCallback(
    (notes: ReleaseNotesView) => {
      if (notes.context === "current") {
        toast.dismiss(whatsNewToastId(notes.version));
        pendingWhatsNew.current = null;
        clearVisibleWhatsNew(notes.version);
      }
      setDialogState({
        open: true,
        version: notes.version,
        body: notes.body,
        context: notes.context,
        channel: notes.channel,
      });
    },
    [clearVisibleWhatsNew],
  );

  const presentWhatsNewToast = useCallback(
    (notes: ReleaseNotesView) => {
      if (isGlobalModalOpen.current) {
        pendingWhatsNew.current = notes;
        return;
      }

      const toastId = whatsNewToastId(notes.version);
      pendingWhatsNew.current = null;
      visibleWhatsNew.current = notes;
      visibleWhatsNewToastId.current = toastId;
      showWhatsNewToast(notes, openReleaseNotes, () => clearVisibleWhatsNew(notes.version));
    },
    [clearVisibleWhatsNew, openReleaseNotes],
  );

  const openCurrentReleaseNotes = useCallback(() => {
    setDialogState({ open: true, context: "current" });
  }, []);

  const checkForUpdates = useCallback(
    async ({
      manual = false,
      force = false,
      target = updateChannel,
    }: CheckForUpdatesOptions = {}) => {
      if (!isUpdateChannelSettled) {
        const queued = queuedUnsettledCheck.current;
        queuedUnsettledCheck.current = {
          manual: manual || queued?.manual === true,
          force: force || queued?.force === true,
        };
        return;
      }

      if (checkInFlight.current) {
        if (force) {
          const queued = queuedForcedCheck.current;
          queuedForcedCheck.current = {
            force: true,
            manual: manual || queued?.manual === true,
            target,
          };
        }
        return;
      }

      const now = Date.now();
      if (
        !manual &&
        !force &&
        lastCheckAt.current !== null &&
        now - lastCheckAt.current < LIFECYCLE_UPDATE_CHECK_COOLDOWN_MS
      ) {
        return;
      }

      checkInFlight.current = true;
      lastCheckAt.current = now;
      const generation = checkGeneration.current;
      if (manual) {
        showCheckingForUpdatesToast();
      }

      try {
        const update = await check({ target });
        if (
          generation !== checkGeneration.current ||
          target !== activeUpdateChannel.current
        ) {
          return;
        }
        const shouldShowManualResult = manual;
        if (
          update &&
          (shouldShowManualResult || notifiedVersion.current !== update.version)
        ) {
          notifiedVersion.current = update.version;
          if (shouldShowManualResult) {
            toast.dismiss(UPDATE_CHECK_RESULT_TOAST_ID);
          }
          showUpdateNotification(update, target, openReleaseNotes, installUpdate);
        } else if (shouldShowManualResult && !update) {
          toast.success(`RalphX is up to date on ${updateChannelLabel(target)}.`, {
            id: UPDATE_CHECK_RESULT_TOAST_ID,
          });
        }
      } catch (error) {
        console.debug("Update check failed:", error);
        if (
          generation === checkGeneration.current &&
          target === activeUpdateChannel.current &&
          manual
        ) {
          toast.error("Failed to check for updates. Please try again later.", {
            id: UPDATE_CHECK_RESULT_TOAST_ID,
          });
        }
      } finally {
        checkInFlight.current = false;
        const queuedCheck = queuedForcedCheck.current;
        queuedForcedCheck.current = null;
        if (queuedCheck !== null) {
          void checkForUpdatesRef.current?.(queuedCheck);
        }
      }
    },
    [isUpdateChannelSettled, openReleaseNotes, updateChannel],
  );

  const handleCheckFromDialog = useCallback(
    (target: UpdateChannel) => {
      if (!isUpdateChannelSettled) {
        return;
      }
      setDialogState({ open: false });
      void checkForUpdates({ force: true, manual: true, target });
    },
    [checkForUpdates, isUpdateChannelSettled],
  );

  useEffect(() => {
    checkForUpdatesRef.current = checkForUpdates;
  }, [checkForUpdates]);

  const showStartupReleaseNotes = useCallback(async () => {
    try {
      const [notes, lastSeenVersion] = await Promise.all([
        getCurrentReleaseNotes(),
        getLastSeenReleaseNotesVersion(),
      ]);
      const body = sanitizeReleaseNotesBody(notes.body);

      if (
        !body ||
        lastSeenVersion === notes.version ||
        whatsNewVersion.current === notes.version
      ) {
        return;
      }

      whatsNewVersion.current = notes.version;
      presentWhatsNewToast({
        version: notes.version,
        body,
        context: "current",
        channel: updateChannel,
      });
    } catch (error) {
      console.debug("Release notes startup check failed:", error);
    }
  }, [presentWhatsNewToast, updateChannel]);

  useEffect(() => {
    isGlobalModalOpen.current = activeModal !== null;
    if (activeModal !== null) {
      if (visibleWhatsNew.current && visibleWhatsNewToastId.current) {
        pendingWhatsNew.current = visibleWhatsNew.current;
        toast.dismiss(visibleWhatsNewToastId.current);
        clearVisibleWhatsNew();
      }
      return;
    }

    if (pendingWhatsNew.current) {
      presentWhatsNewToast(pendingWhatsNew.current);
    }
  }, [activeModal, clearVisibleWhatsNew, presentWhatsNewToast]);

  useEffect(() => {
    if (!isUpdateChannelError) {
      return;
    }
    console.debug(
      "Update channel load failed; using Stable for update checks:",
      updateChannelError,
    );
  }, [isUpdateChannelError, updateChannelError]);

  useEffect(() => {
    if (!isUpdateChannelSettled) {
      return;
    }

    if (activeUpdateChannel.current === null) {
      activeUpdateChannel.current = updateChannel;
      return;
    }
    if (activeUpdateChannel.current === updateChannel) {
      return;
    }

    activeUpdateChannel.current = updateChannel;
    checkGeneration.current += 1;
    notifiedVersion.current = null;
    lastCheckAt.current = null;
    toast.dismiss("update-available");
    toast.dismiss(UPDATE_CHECK_RESULT_TOAST_ID);

    if (!automaticMaintenanceEnabled) {
      return;
    }

    if (checkInFlight.current) {
      queuedForcedCheck.current = { force: true, target: updateChannel };
      return;
    }
    void checkForUpdates({ force: true, target: updateChannel });
  }, [
    automaticMaintenanceEnabled,
    checkForUpdates,
    isUpdateChannelSettled,
    updateChannel,
  ]);

  useEffect(() => {
    if (!isUpdateChannelSettled || queuedUnsettledCheck.current === null) {
      return;
    }
    const queued = queuedUnsettledCheck.current;
    queuedUnsettledCheck.current = null;
    void checkForUpdates({ ...queued, target: updateChannel });
  }, [checkForUpdates, isUpdateChannelSettled, updateChannel]);

  useEffect(() => {
    if (!automaticMaintenanceEnabled || !isUpdateChannelSettled) {
      return undefined;
    }
    const timeoutId = window.setTimeout(
      () => void checkForUpdatesRef.current?.({ force: true }),
      INITIAL_UPDATE_CHECK_DELAY_MS,
    );
    const intervalId = window.setInterval(
      () => void checkForUpdatesRef.current?.({ force: true }),
      UPDATE_CHECK_INTERVAL_MS,
    );
    return () => {
      window.clearTimeout(timeoutId);
      window.clearInterval(intervalId);
    };
  }, [automaticMaintenanceEnabled, isUpdateChannelSettled]);

  useEffect(() => {
    if (!automaticMaintenanceEnabled) {
      return undefined;
    }

    const timeoutId = window.setTimeout(
      () => void showStartupReleaseNotes(),
      STARTUP_RELEASE_NOTES_DELAY_MS,
    );
    return () => window.clearTimeout(timeoutId);
  }, [automaticMaintenanceEnabled, showStartupReleaseNotes]);

  useEffect(() => {
    if (!automaticMaintenanceEnabled) {
      return undefined;
    }

    const checkIfActive = () => {
      if (document.visibilityState === "hidden") return;
      void checkForUpdates();
    };

    window.addEventListener("focus", checkIfActive);
    window.addEventListener("online", checkIfActive);
    document.addEventListener("visibilitychange", checkIfActive);

    return () => {
      window.removeEventListener("focus", checkIfActive);
      window.removeEventListener("online", checkIfActive);
      document.removeEventListener("visibilitychange", checkIfActive);
    };
  }, [automaticMaintenanceEnabled, checkForUpdates]);

  useEffect(() => {
    if (checkForUpdatesRequest <= handledCheckForUpdatesRequest.current) {
      return;
    }
    handledCheckForUpdatesRequest.current = checkForUpdatesRequest;
    void checkForUpdates({ manual: true, force: true });
  }, [checkForUpdates, checkForUpdatesRequest]);

  useEffect(() => {
    if (openReleaseNotesRequest <= handledOpenReleaseNotesRequest.current) {
      return;
    }
    handledOpenReleaseNotesRequest.current = openReleaseNotesRequest;
    openCurrentReleaseNotes();
  }, [openCurrentReleaseNotes, openReleaseNotesRequest]);

  useUpdateCheckerNativeEvents({
    checkForUpdates,
    enabled: listenForNativeActions,
    openCurrentReleaseNotes,
  });


  return (
    <ReleaseNotesDialog
      open={dialogState.open}
      onClose={() => setDialogState({ open: false })}
      initialVersion={dialogState.version}
      initialBody={dialogState.body}
      initialContext={dialogState.context}
      initialChannel={dialogState.channel}
      onCheckForUpdates={handleCheckFromDialog}
    />
  );
}

function showCheckingForUpdatesToast() {
  toast.loading("Checking for updates...", { id: UPDATE_CHECK_RESULT_TOAST_ID });
}
