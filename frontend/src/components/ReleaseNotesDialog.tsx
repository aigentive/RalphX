import {
  lazy,
  memo,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { ArrowUpCircle, FileText, Loader2 } from "lucide-react";
import { getVersion } from "@tauri-apps/api/app";

import {
  compareSemverDesc,
  fetchReleaseMetadata,
  getReleaseNotesForVersion,
  listReleaseNotesVersions,
  mergeVersionLists,
} from "@/api/release-notes";
import type { ReleaseMetadata } from "@/api/release-notes";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";

const GITHUB_RELEASE_METADATA_MARKERS =
  /^[ \t]*<!--\s*github-release-metadata:(?:start|end)\s*-->[ \t]*\n?/gm;

interface ReleaseNotesDialogProps {
  open: boolean;
  onClose: () => void;
  initialVersion?: string | undefined;
  initialBody?: string | null | undefined;
  initialContext?: "current" | "update" | undefined;
  onRequestUpdate?: () => void;
}

const LazyMarkdown = lazy(async () => {
  const [{ default: ReactMarkdown }, { default: remarkGfm }] =
    await Promise.all([import("react-markdown"), import("remark-gfm")]);

  return {
    default: memo(function ReleaseMarkdown({ body }: { body: string }) {
      return (
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          components={markdownComponents}
          skipHtml
        >
          {body}
        </ReactMarkdown>
      );
    }),
  };
});

function sanitize(body: string | null): string | null {
  if (!body) return null;
  const result = body
    .replace(GITHUB_RELEASE_METADATA_MARKERS, "")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
  return result.length > 0 ? result : null;
}

function formatMonthYear(dateStr: string): string {
  const d = new Date(dateStr);
  return d.toLocaleDateString("en-US", { month: "long", year: "numeric" });
}

function formatDay(dateStr: string): string {
  const d = new Date(dateStr);
  return d.toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

function isNewerThan(version: string, current: string): boolean {
  return compareSemverDesc(version, current) < 0;
}

type SidebarItem =
  | { kind: "header"; label: string }
  | { kind: "version"; version: string; date: string | null; isCurrent: boolean };

function buildSidebarItems(
  versions: string[],
  metadata: Map<string, ReleaseMetadata>,
  currentAppVersion: string | null,
): SidebarItem[] {
  const items: SidebarItem[] = [];
  let currentMonth = "";

  for (const version of versions) {
    const meta = metadata.get(version);
    const monthYear = meta ? formatMonthYear(meta.publishedAt) : null;

    if (monthYear && monthYear !== currentMonth) {
      currentMonth = monthYear;
      items.push({ kind: "header", label: monthYear });
    }

    items.push({
      kind: "version",
      version,
      date: meta ? formatDay(meta.publishedAt) : null,
      isCurrent: version === currentAppVersion,
    });
  }

  return items;
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
  onRequestUpdate,
}: ReleaseNotesDialogProps) {
  const [versions, setVersions] = useState<string[]>([]);
  const [loadedNotes, setLoadedNotes] = useState<
    Record<string, string | null>
  >({});
  const [activeVersion, setActiveVersion] = useState<string | null>(null);
  const [activeLoading, setActiveLoading] = useState(false);
  const [listLoading, setListLoading] = useState(false);
  const [metadata, setMetadata] = useState<Map<string, ReleaseMetadata>>(
    new Map(),
  );
  const [bundledVersions, setBundledVersions] = useState<Set<string>>(
    new Set(),
  );
  const [currentAppVersion, setCurrentAppVersion] = useState<string | null>(
    null,
  );

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setListLoading(true);

    void Promise.all([
      listReleaseNotesVersions(),
      fetchReleaseMetadata(),
      getVersion().catch(() => null),
    ])
      .then(([bundled, meta, appVersion]) => {
        if (cancelled) return;
        const bundledSet = new Set(bundled);
        setBundledVersions(bundledSet);
        setMetadata(meta);
        if (appVersion) setCurrentAppVersion(appVersion);

        const merged = includeSeededVersion(
          mergeVersionLists(bundled, meta),
          initialVersion,
          initialBody,
        );
        setVersions(merged);
        setListLoading(false);

        const first = merged[0];
        if (first === undefined) return;

        const target = initialVersion
          ? (merged.find((v) => v === initialVersion) ?? first)
          : first;
        setActiveVersion(target);

        const seededBody =
          target === initialVersion && initialBody !== undefined
            ? sanitize(initialBody)
            : undefined;
        if (seededBody !== undefined) {
          setLoadedNotes((prev) => ({
            ...prev,
            [target]: seededBody,
          }));
          return;
        }

        if (bundledSet.has(target)) {
          setActiveLoading(true);
          void getReleaseNotesForVersion(target)
            .then((resp) => {
              if (!cancelled) {
                setLoadedNotes((prev) => ({
                  ...prev,
                  [target]: sanitize(resp.body),
                }));
              }
            })
            .catch(() => {
              if (!cancelled) {
                const ghBody = meta.get(target)?.body ?? null;
                setLoadedNotes((prev) => ({
                  ...prev,
                  [target]: sanitize(ghBody),
                }));
              }
            })
            .finally(() => {
              if (!cancelled) setActiveLoading(false);
            });
        } else {
          const ghBody = meta.get(target)?.body ?? null;
          setLoadedNotes((prev) => ({
            ...prev,
            [target]: sanitize(ghBody),
          }));
        }
      })
      .catch(() => {
        if (!cancelled) setListLoading(false);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  useEffect(() => {
    if (!open) {
      setVersions([]);
      setLoadedNotes({});
      setActiveVersion(null);
      setActiveLoading(false);
      setMetadata(new Map());
      setBundledVersions(new Set());
      setCurrentAppVersion(null);
    }
  }, [open]);

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

  const contextLabel = useMemo(() => {
    if (initialContext === "update") return "Available update";
    return "Release History";
  }, [initialContext]);

  const sidebarItems = useMemo(
    () => buildSidebarItems(versions, metadata, currentAppVersion),
    [versions, metadata, currentAppVersion],
  );

  const updateAvailable = useMemo(() => {
    if (!currentAppVersion || versions.length === 0) return false;
    const first = versions[0];
    return first !== undefined && isNewerThan(first, currentAppVersion);
  }, [currentAppVersion, versions]);

  const activeBody = activeVersion ? loadedNotes[activeVersion] : undefined;
  const activeMeta = activeVersion ? metadata.get(activeVersion) : undefined;

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        className="flex max-h-[85vh] min-h-[60vh] max-w-4xl flex-col overflow-hidden p-0"
        style={{
          backgroundColor: "var(--dialog-bg, var(--bg-elevated))",
          borderColor: "var(--border-subtle)",
        }}
      >
        <DialogHeader
          className="shrink-0 border-b px-6 py-4"
          style={{ borderColor: "var(--border-subtle)" }}
        >
          <div className="flex min-w-0 items-center justify-between gap-3">
            <div className="flex min-w-0 items-center gap-2">
              <FileText
                className="h-4 w-4 shrink-0"
                style={{ color: "var(--accent-primary)" }}
              />
              <div className="min-w-0">
                <DialogTitle className="truncate">Release Notes</DialogTitle>
                <DialogDescription>{contextLabel}</DialogDescription>
              </div>
            </div>
            {updateAvailable && onRequestUpdate && (
              <button
                type="button"
                data-testid="release-notes-update-button"
                className="inline-flex shrink-0 items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-semibold transition-opacity hover:opacity-90"
                style={{
                  backgroundColor: "var(--accent-primary)",
                  color: "white",
                }}
                onClick={onRequestUpdate}
              >
                <ArrowUpCircle className="h-3.5 w-3.5" />
                Update to v{versions[0]}
              </button>
            )}
          </div>
        </DialogHeader>

        <div className="flex min-h-0 flex-1">
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

const VersionSidebar = memo(function VersionSidebar({
  items,
  activeVersion,
  loading,
  onClick,
}: {
  items: SidebarItem[];
  activeVersion: string | null;
  loading: boolean;
  onClick: (version: string) => void;
}) {
  const activeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    activeRef.current?.scrollIntoView?.({ block: "nearest" });
  }, [activeVersion]);

  if (loading) {
    return (
      <div
        className="flex w-52 shrink-0 items-center justify-center border-l"
        style={{
          borderColor: "var(--border-subtle)",
          backgroundColor: "var(--bg-surface)",
        }}
      >
        <Loader2
          className="h-4 w-4 animate-spin"
          style={{ color: "var(--text-muted)" }}
        />
      </div>
    );
  }

  return (
    <div
      className="w-52 shrink-0 overflow-y-auto border-l"
      style={{
        borderColor: "var(--border-subtle)",
        backgroundColor: "var(--bg-surface)",
      }}
    >
      <div className="py-2">
        {items.map((item, i) => {
          if (item.kind === "header") {
            return (
              <div
                key={`h-${item.label}`}
                className="px-4 pb-1 text-[0.6875rem] font-semibold uppercase tracking-wider"
                style={{
                  color: "var(--text-muted)",
                  paddingTop: i === 0 ? "4px" : "12px",
                }}
              >
                {item.label}
              </div>
            );
          }

          const isActive = item.version === activeVersion;
          return (
            <button
              key={item.version}
              ref={isActive ? activeRef : undefined}
              type="button"
              className="flex w-full items-center justify-between gap-2 rounded-none px-4 py-1.5 text-left transition-colors"
              style={{
                color: isActive
                  ? "var(--accent-primary)"
                  : "var(--text-secondary)",
                backgroundColor: isActive
                  ? "var(--bg-elevated)"
                  : "transparent",
                borderLeft: isActive
                  ? "2px solid var(--accent-primary)"
                  : "2px solid transparent",
              }}
              onClick={() => onClick(item.version)}
            >
              <span className="truncate text-[0.8125rem] font-medium">
                v{item.version}
              </span>
              {item.isCurrent ? (
                <span
                  className="shrink-0 text-[0.6875rem] font-semibold"
                  style={{ color: "var(--accent-primary)" }}
                >
                  current
                </span>
              ) : item.date ? (
                <span
                  className="shrink-0 text-[0.6875rem]"
                  style={{ color: "var(--text-muted)" }}
                >
                  {item.date}
                </span>
              ) : null}
            </button>
          );
        })}
      </div>
    </div>
  );
});

const VersionContent = memo(function VersionContent({
  version,
  body,
  loading,
  date,
}: {
  version: string;
  body: string | null | undefined;
  loading: boolean;
  date: string | null;
}) {
  return (
    <div className="px-8 py-6">
      <div className="mb-4 flex items-baseline gap-3">
        <h2
          className="text-lg font-semibold"
          style={{ color: "var(--text-primary)" }}
        >
          v{version}
        </h2>
        {date && (
          <span
            className="text-[0.75rem]"
            style={{ color: "var(--text-muted)" }}
          >
            {formatDay(date)}
          </span>
        )}
      </div>

      {loading || body === undefined ? (
        <div className="flex items-center gap-2 py-8">
          <Loader2
            className="h-4 w-4 animate-spin"
            style={{ color: "var(--text-muted)" }}
          />
          <span className="text-sm" style={{ color: "var(--text-muted)" }}>
            Loading...
          </span>
        </div>
      ) : body ? (
        <div
          className="text-[0.8125rem] leading-relaxed"
          style={{ color: "var(--text-primary)" }}
        >
          <Suspense
            fallback={
              <pre className="whitespace-pre-wrap font-sans">{body}</pre>
            }
          >
            <LazyMarkdown body={body} />
          </Suspense>
        </div>
      ) : (
        <p className="py-4 text-sm" style={{ color: "var(--text-muted)" }}>
          Release notes not available for this version.
        </p>
      )}
    </div>
  );
});
