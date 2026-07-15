import { lazy, Suspense, useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ArrowLeft, ChevronDown, FilePlus2, Files, FolderOpen } from "lucide-react";
import { z } from "zod";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useAfterPaintMounted } from "@/components/agents/agentDeferredFrame";
import {
  personaKeys,
  useApprovePersona,
  useIngestPersonaContext,
  usePersona,
  usePersonaBuilderIngestStatus,
} from "@/hooks/usePersonas";
import { usePersonaDraftEvents } from "@/hooks/usePersonaDraftEvents";
import type { PersonaIngestManifest } from "@/types/persona";

const LazyIntegratedChatPanel = lazy(() =>
  import("@/components/Chat/IntegratedChatPanel").then((module) => ({
    default: module.IntegratedChatPanel,
  })),
);

const LazyPersonaMarkdown = lazy(async () => {
  const [{ default: ReactMarkdown }, { default: remarkGfm }] = await Promise.all([
    import("react-markdown"),
    import("remark-gfm"),
  ]);
  return {
    default: function PersonaMarkdown({ content }: { content: string }) {
      return <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>;
    },
  };
});

const BuilderConversationSchema = z.object({ id: z.string().min(1) });

function ErrorNotice({ message }: { message: string }) {
  return (
    <div
      role="alert"
      className="rounded-md border border-[var(--status-error-border)] bg-[var(--status-error-muted)] px-3 py-2 text-sm text-[var(--status-error)]"
    >
      {message}
    </div>
  );
}

function ManifestEntries({ entries }: { entries: PersonaIngestManifest["rejected"] }) {
  return entries.length > 0 ? (
    <ul className="mt-2 space-y-1 text-xs text-[var(--text-secondary)]">
      {entries.map((entry) => (
        <li key={`${entry.path}:${entry.reason ?? ""}`}>
          {entry.path}{entry.reason ? `: ${entry.reason}` : ""}
        </li>
      ))}
    </ul>
  ) : null;
}

function PersonaContextPicker({
  className,
  disabled,
  isAddingContext,
  onAddFiles,
  onAddFolder,
  size = "default",
  testId,
}: {
  className?: string;
  disabled: boolean;
  isAddingContext: boolean;
  onAddFiles: () => void;
  onAddFolder: () => void;
  size?: "default" | "sm";
  testId?: string;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="outline"
          size={size}
          className={className}
          disabled={disabled}
          data-testid={testId}
        >
          <FilePlus2 aria-hidden="true" />
          {isAddingContext ? "Adding…" : "Add context…"}
          <ChevronDown aria-hidden="true" className="size-3.5" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="min-w-44">
        <DropdownMenuItem className="cursor-pointer" onSelect={onAddFiles}>
          <Files aria-hidden="true" />
          Add files
        </DropdownMenuItem>
        <DropdownMenuItem className="cursor-pointer" onSelect={onAddFolder}>
          <FolderOpen aria-hidden="true" />
          Add folder
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function PersonaBuilderContextGate({
  isAddingContext,
  onAddFiles,
  onAddFolder,
}: {
  isAddingContext: boolean;
  onAddFiles: () => void;
  onAddFolder: () => void;
}) {
  return (
    <div className="flex h-full flex-col items-center justify-center px-6 text-center">
      <FilePlus2 aria-hidden="true" className="size-8 text-[var(--text-muted)]" />
      <h4 className="mt-4 text-base font-semibold text-[var(--text-primary)]">
        Add context to start
      </h4>
      <p className="mt-2 max-w-sm text-sm text-[var(--text-secondary)]">
        The builder agent can only read files you add. Add project docs, style guides, or examples,
        then describe the persona you want.
      </p>
      <PersonaContextPicker
        className="mt-5"
        disabled={isAddingContext}
        isAddingContext={isAddingContext}
        onAddFiles={onAddFiles}
        onAddFolder={onAddFolder}
        testId="persona-builder-empty-add-context"
      />
    </div>
  );
}

export function PersonaBuilderView({
  projectId,
  onBack,
}: {
  projectId: string;
  onBack: () => void;
}) {
  const queryClient = useQueryClient();
  const [conversationId, setConversationId] = useState<string | null>(null);
  const [creationError, setCreationError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const draftId = usePersonaDraftEvents();
  const { data: draft, isLoading: isDraftLoading } = usePersona(draftId ?? "");
  const approvePersona = useApprovePersona();
  const ingestContext = useIngestPersonaContext();
  const chatReady = useAfterPaintMounted(true);
  const ingestStatus = usePersonaBuilderIngestStatus(conversationId);
  const draftPreviewReady = useAfterPaintMounted(Boolean(draft));

  useEffect(() => {
    if (!chatReady || conversationId || creationError) {
      return;
    }
    let cancelled = false;
    void invoke<unknown>("create_persona_builder_conversation", {
      input: { projectId },
    })
      .then((response) => BuilderConversationSchema.parse(response))
      .then((conversation) => {
        if (!cancelled) {
          setConversationId(conversation.id);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setCreationError(error instanceof Error ? error.message : String(error));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [chatReady, conversationId, creationError, projectId]);

  const handleAddContext = async (kind: "files" | "folder") => {
    if (!conversationId) {
      return;
    }
    setActionError(null);
    try {
      const picked = await openDialog(
        kind === "files"
          ? {
              directory: false,
              multiple: true,
              title: "Add persona context files",
            }
          : {
              directory: true,
              multiple: false,
              title: "Add persona context folder",
            },
      );
      const pickedPaths = Array.isArray(picked)
        ? picked.filter((path): path is string => typeof path === "string" && path.length > 0)
        : typeof picked === "string" && picked.length > 0
          ? [picked]
          : [];
      if (pickedPaths.length === 0) {
        return;
      }
      await ingestContext.mutateAsync({ conversationId, pickedPaths });
    } catch (error) {
      setActionError(error instanceof Error ? error.message : String(error));
    }
  };

  const handleApprove = async () => {
    if (!draft) {
      return;
    }
    setActionError(null);
    try {
      await approvePersona.mutateAsync(draft.id);
      await queryClient.invalidateQueries({ queryKey: personaKeys.list() });
      onBack();
    } catch (error) {
      setActionError(error instanceof Error ? error.message : String(error));
    }
  };

  const manifest = ingestContext.data;
  const isApproving = approvePersona.isPending;
  return (
    <section aria-label="Persona Builder" className="flex h-full min-h-[520px] flex-col gap-4">
      <div className="flex items-center gap-2">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label="Back to personas"
              onClick={onBack}
            >
              <ArrowLeft aria-hidden="true" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Back to personas</TooltipContent>
        </Tooltip>
        <div>
          <h3 className="text-base font-semibold text-[var(--text-primary)]">Persona Builder</h3>
          <p className="text-sm text-[var(--text-secondary)]">Build a reusable agent voice with guided context.</p>
        </div>
      </div>

      <div className="grid min-h-0 flex-1 gap-4 lg:grid-cols-[minmax(0,1.2fr)_minmax(280px,0.8fr)]">
        <div className="min-h-[360px] overflow-hidden rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]">
          {creationError ? (
            <div className="p-4"><ErrorNotice message={creationError} /></div>
          ) : !chatReady || !conversationId || ingestStatus.isPending ? (
            <div className="h-full animate-pulse bg-[var(--bg-elevated)]" aria-label="Loading builder chat" />
          ) : ingestStatus.data?.live === true ? (
            <Suspense fallback={<div className="h-full animate-pulse bg-[var(--bg-elevated)]" aria-label="Loading builder chat" />}>
              <LazyIntegratedChatPanel
                projectId={projectId}
                conversationIdOverride={conversationId}
                hideSessionToolbar
                hideHeaderSessionControls
                contentWidthClassName="max-w-none"
              />
            </Suspense>
          ) : (
            <PersonaBuilderContextGate
              isAddingContext={ingestContext.isPending}
              onAddFiles={() => void handleAddContext("files")}
              onAddFolder={() => void handleAddContext("folder")}
            />
          )}
        </div>

        <aside className="space-y-4 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-4">
          <div>
            <h4 className="text-sm font-semibold text-[var(--text-primary)]">Draft preview</h4>
            <div className="mt-2 min-h-40 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-3">
              {draft ? (
                draftPreviewReady ? (
                  <Suspense fallback={<div className="h-24 animate-pulse rounded bg-[var(--bg-hover)]" aria-label="Loading draft preview" />}>
                    <LazyPersonaMarkdown content={draft.content} />
                  </Suspense>
                ) : (
                  <div className="h-24 animate-pulse rounded bg-[var(--bg-hover)]" aria-label="Loading draft preview" />
                )
              ) : (
                <div className="h-24 animate-pulse rounded bg-[var(--bg-hover)]" aria-label="Loading draft preview" />
              )}
            </div>
            {draftId && isDraftLoading && <p className="mt-2 text-xs text-[var(--text-muted)]">Refreshing draft…</p>}
          </div>

          <div>
            <h4 className="text-sm font-semibold text-[var(--text-primary)]">Ingested context</h4>
            {manifest ? (
              <>
                <p className="mt-2 text-xs text-[var(--text-secondary)]">
                  ✓ {manifest.copied.length} copied · {manifest.skipped.length} skipped
                </p>
                <p className="mt-1 text-xs text-[var(--status-warning)]">⚠ {manifest.rejected.length} rejected</p>
                <ManifestEntries entries={manifest.skipped} />
                <ManifestEntries entries={manifest.rejected} />
              </>
            ) : (
              <p className="mt-2 text-xs text-[var(--text-muted)]">Add project context when it will improve the persona.</p>
            )}
          </div>

          {actionError && <ErrorNotice message={actionError} />}
          <div className="flex flex-wrap gap-2">
            <PersonaContextPicker
              disabled={!conversationId || ingestContext.isPending}
              isAddingContext={ingestContext.isPending}
              onAddFiles={() => void handleAddContext("files")}
              onAddFolder={() => void handleAddContext("folder")}
              size="sm"
            />
            <Button type="button" size="sm" onClick={() => void handleApprove()} disabled={!draft || isApproving} className="bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-secondary)]">
              {isApproving ? "Approving…" : "Approve"}
            </Button>
          </div>
        </aside>
      </div>
    </section>
  );
}
