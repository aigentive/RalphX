import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";

import {
  compareSemverDesc,
  fetchReleaseMetadata,
  getReleaseNotesForVersion,
  listReleaseNotesVersions,
  mergeVersionLists,
} from "@/api/release-notes";
import type {
  ReleaseMetadata,
  ReleaseMetadataAvailability,
} from "@/api/release-notes";
import type { UpdateChannel } from "@/api/update-channel";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { useUpdateChannel } from "@/hooks/useUpdateChannel";
import { ReleaseNotesChannels } from "./ReleaseNotesDialog.channels";
import { VersionContent } from "./ReleaseNotesDialog.content";
import { buildSidebarItems } from "./ReleaseNotesDialog.sidebar-items";
import { VersionSidebar } from "./ReleaseNotesDialog.sidebar";

const GITHUB_RELEASE_METADATA_MARKERS =
  /^[ \t]*<!--\s*github-release-metadata:(?:start|end)\s*-->[ \t]*\n?/gm;

interface ReleaseNotesDialogProps {
  open: boolean;
  onClose: () => void;
  initialVersion?: string | undefined;
  initialBody?: string | null | undefined;
  initialContext?: "current" | "update" | undefined;
  initialChannel?: UpdateChannel | undefined;
  onCheckForUpdates?: (channel: UpdateChannel) => void;
}

function sanitize(body: string | null): string | null {
  if (!body) return null;
  const result = body
    .replace(GITHUB_RELEASE_METADATA_MARKERS, "")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
  return result.length > 0 ? result : null;
}

function includeSeededVersion(
  versions: string[],
  initialVersion: string | undefined,
  initialBody: string | null | undefined,
): string[] {
  if (initialVersion === undefined || initialBody === undefined) {
    return versions;
  }

  if (versions.includes(initialVersion)) {
    return versions;
  }

  return [...versions, initialVersion].sort(compareSemverDesc);
}

export function ReleaseNotesDialog({
  open,
  onClose,
  initialVersion,
  initialBody,
  initialContext,
  initialChannel,
  onCheckForUpdates,
}: ReleaseNotesDialogProps) {
  const {
    updateChannel,
    isSettled: isUpdateChannelSettled,
    isLoading: isUpdateChannelLoading,
    isError: isUpdateChannelError,
    setUpdateChannel,
    isSaving: isUpdateChannelSaving,
    saveError: updateChannelSaveError,
  } = useUpdateChannel();
  const [browseChannel, setBrowseChannel] = useState<UpdateChannel | null>(null);
  const hasUserBrowsed = useRef(false);
  const [loadedNotes, setLoadedNotes] = useState<
    Record<string, string | null>
  >({});
  const [activeVersion, setActiveVersion] = useState<string | null>(null);
  const [activeLoading, setActiveLoading] = useState(false);
  const [listLoading, setListLoading] = useState(false);
  const [metadata, setMetadata] = useState<Map<string, ReleaseMetadata>>(
    new Map(),
  );
  const [metadataAvailability, setMetadataAvailability] =
    useState<ReleaseMetadataAvailability | null>(null);
  const [bundledVersionList, setBundledVersionList] = useState<string[]>([]);
  const [currentAppVersion, setCurrentAppVersion] = useState<string | null>(
    null,
  );

  useEffect(() => {
    if (!open) return;
    setBrowseChannel((current) => {
      if (current !== null) return current;
      if (initialChannel !== undefined) return initialChannel;
      return isUpdateChannelSettled ? updateChannel : null;
    });
  }, [initialChannel, isUpdateChannelSettled, open, updateChannel]);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setListLoading(true);

    void Promise.all([
      listReleaseNotesVersions().catch(() => []),
      fetchReleaseMetadata(),
      getVersion().catch(() => null),
    ])
      .then(([bundled, meta, appVersion]) => {
        if (cancelled) return;
        setBundledVersionList(bundled);
        setMetadata(meta);
        setMetadataAvailability(meta.availability ?? "available");
        if (appVersion) setCurrentAppVersion(appVersion);
        setListLoading(false);
      })
      .catch(() => {
        if (!cancelled) setListLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  useEffect(() => {
    if (!open) {
      setLoadedNotes({});
      setActiveVersion(null);
      setActiveLoading(false);
      setMetadata(new Map());
      setMetadataAvailability(null);
      setBundledVersionList([]);
      setCurrentAppVersion(null);
      setBrowseChannel(null);
      hasUserBrowsed.current = false;
    }
  }, [open]);

  const bundledVersions = useMemo(
    () => new Set(bundledVersionList),
    [bundledVersionList],
  );

  const versionsByChannel = useMemo(() => {
    if (
      metadataAvailability === null ||
      metadataAvailability === "unavailable"
    ) {
      return { stable: [], nightly: [] };
    }
    return {
      stable: mergeVersionLists(bundledVersionList, metadata, "stable"),
      nightly: mergeVersionLists(bundledVersionList, metadata, "nightly"),
    };
  }, [bundledVersionList, metadata, metadataAvailability]);

  const seededChannel = useMemo(() => {
    if (initialVersion !== undefined) {
      const classifiedRelease = metadata.get(initialVersion);
      if (classifiedRelease !== undefined) {
        return classifiedRelease.prerelease ? "nightly" : "stable";
      }
    }
    return initialChannel ?? updateChannel;
  }, [initialChannel, initialVersion, metadata, updateChannel]);

  useEffect(() => {
    if (
      !open ||
      metadataAvailability === null ||
      initialVersion === undefined ||
      hasUserBrowsed.current
    ) {
      return;
    }
    setBrowseChannel(seededChannel);
  }, [initialVersion, metadataAvailability, open, seededChannel]);

  const versions = useMemo(() => {
    if (browseChannel === null) return [];
    if (browseChannel !== seededChannel) {
      return versionsByChannel[browseChannel];
    }
    return includeSeededVersion(
      versionsByChannel[browseChannel],
      initialVersion,
      initialBody,
    );
  }, [
    browseChannel,
    initialBody,
    initialVersion,
    seededChannel,
    versionsByChannel,
  ]);

  const handleVersionClick = useCallback(
    (version: string) => {
      setActiveVersion(version);
      if (loadedNotes[version] !== undefined) return;

      if (bundledVersions.has(version)) {
        setActiveLoading(true);
        void getReleaseNotesForVersion(version)
          .then((resp) => {
            setLoadedNotes((prev) => ({
              ...prev,
              [version]: sanitize(resp.body),
            }));
          })
          .catch(() => {
            const ghBody = metadata.get(version)?.body ?? null;
            setLoadedNotes((prev) => ({
              ...prev,
              [version]: sanitize(ghBody),
            }));
          })
          .finally(() => setActiveLoading(false));
      } else {
        const ghBody = metadata.get(version)?.body ?? null;
        setLoadedNotes((prev) => ({
          ...prev,
          [version]: sanitize(ghBody),
        }));
      }
    },
    [loadedNotes, bundledVersions, metadata],
  );

  useEffect(() => {
    if (listLoading || browseChannel === null) return;
    const first = versions[0];
    if (first === undefined) {
      setActiveVersion(null);
      setActiveLoading(false);
      return;
    }
    const target =
      browseChannel === seededChannel &&
      initialVersion !== undefined &&
      versions.includes(initialVersion)
        ? initialVersion
        : first;
    handleVersionClick(target);
    // Selection is reset only when the browsed channel/version list changes.
    // Note-loading state must not jump the user's explicit sidebar selection.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    browseChannel,
    initialVersion,
    listLoading,
    seededChannel,
    versions,
  ]);

  useEffect(() => {
    if (
      !open ||
      initialVersion === undefined ||
      initialBody === undefined ||
      browseChannel !== seededChannel
    ) {
      return;
    }
    setLoadedNotes((current) => ({
      ...current,
      [initialVersion]: sanitize(initialBody),
    }));
  }, [
    browseChannel,
    initialBody,
    initialVersion,
    open,
    seededChannel,
  ]);

  const handleBrowseChannel = useCallback((channel: UpdateChannel) => {
    hasUserBrowsed.current = true;
    setActiveVersion(null);
    setActiveLoading(false);
    setBrowseChannel(channel);
  }, []);

  const handleUseBrowseChannel = useCallback(() => {
    if (
      browseChannel === null ||
      browseChannel === updateChannel ||
      isUpdateChannelSaving
    ) {
      return;
    }
    setUpdateChannel(browseChannel, { onSuccess: onClose });
  }, [
    browseChannel,
    isUpdateChannelSaving,
    onClose,
    setUpdateChannel,
    updateChannel,
  ]);

  const contextLabel = useMemo(() => {
    if (initialContext === "update") return "Available update";
    return "Release History";
  }, [initialContext]);

  const sidebarItems = useMemo(
    () => buildSidebarItems(versions, metadata, currentAppVersion),
    [versions, metadata, currentAppVersion],
  );

  const activeBody = activeVersion ? loadedNotes[activeVersion] : undefined;
  const activeMeta = activeVersion ? metadata.get(activeVersion) : undefined;
  const isBrowsingPersistedChannel =
    browseChannel !== null && browseChannel === updateChannel;

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        aria-labelledby="release-notes-dialog-title"
        aria-describedby="release-notes-dialog-description"
        className="flex max-h-[85vh] min-h-[60vh] max-w-4xl flex-col overflow-hidden p-0"
        style={{
          backgroundColor: "var(--dialog-bg, var(--bg-elevated))",
          borderColor: "var(--border-subtle)",
        }}
      >
        <ReleaseNotesChannels
          browseChannel={browseChannel}
          contextLabel={contextLabel}
          initialChannel={initialChannel}
          isBrowsingPersistedChannel={isBrowsingPersistedChannel}
          isUpdateChannelError={isUpdateChannelError}
          isUpdateChannelLoading={isUpdateChannelLoading}
          isUpdateChannelSaving={isUpdateChannelSaving}
          isUpdateChannelSettled={isUpdateChannelSettled}
          metadataAvailability={metadataAvailability}
          onBrowseChannel={handleBrowseChannel}
          onCheckForUpdates={onCheckForUpdates}
          onUseBrowseChannel={handleUseBrowseChannel}
          updateChannel={updateChannel}
          updateChannelSaveError={updateChannelSaveError}
          versionsByChannel={versionsByChannel}
        />

        <div
          className="flex min-h-0 flex-1"
          role="tabpanel"
          id="release-notes-panel"
          aria-labelledby={`release-notes-tab-${browseChannel ?? "stable"}`}
        >
          <div
            className="flex-1 overflow-y-auto"
            data-testid="release-notes-dialog-body"
          >
            {activeVersion && (
              <VersionContent
                version={activeVersion}
                body={activeBody}
                loading={activeLoading && activeBody === undefined}
                date={activeMeta?.publishedAt ?? null}
              />
            )}
          </div>

          <VersionSidebar
            items={sidebarItems}
            activeVersion={activeVersion}
            loading={listLoading}
            onClick={handleVersionClick}
          />
        </div>
      </DialogContent>
    </Dialog>
  );
}
