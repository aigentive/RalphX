import { useCallback, useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { AlertCircle, FileText, Loader2, Search, Upload } from "lucide-react";

import { artifactApi } from "@/api/artifact";
import type { AgentComposerPlanReference } from "@/api/agent-composer";
import { chatApi, type AgentConversationPlanDraft } from "@/api/chat";
import { DropZoneOverlay } from "@/components/Ideation/DropZoneOverlay";
import { Button } from "@/components/ui/button";
import { useConfirmation } from "@/hooks/useConfirmation";
import { useAgentComposerPlanReferences } from "@/hooks/useAgentComposerResources";
import { useFileDrop } from "@/hooks/useFileDrop";
import { cn } from "@/lib/utils";
import { AgentPlanStartPanelPreview } from "./AgentPlanStartPanelPreview";
import { AgentPlanStartPanelSearch } from "./AgentPlanStartPanelSearch";
import { planTitle, samePlanReference } from "./AgentPlanStartPanel.utils";

type AgentPlanStartPanelStatus = "idle" | "loading" | "error" | "pending";

interface AgentPlanStartPanelProps {
  status?: AgentPlanStartPanelStatus;
  errorMessage?: string | null;
  projectId?: string | null;
  conversationId?: string | null;
  onDraftCreated?: (draft: AgentConversationPlanDraft) => void | Promise<void>;
}

const STATUS_COPY: Record<
  AgentPlanStartPanelStatus,
  { label: string; detail: string }
> = {
  idle: {
    label: "No plan selected",
    detail: "Search project plans or import markdown to create a draft plan.",
  },
  loading: {
    label: "Loading plans...",
    detail: "Checking for an existing plan before showing draft options.",
  },
  error: {
    label: "Plan setup unavailable",
    detail: "The plan surface is available, but plan setup could not finish.",
  },
  pending: {
    label: "Preparing draft plan...",
    detail: "Plan setup is still settling for this conversation.",
  },
};

const PLAN_IMPORT_MAX_BYTES = 1024 * 1024;
const EMPTY_PLAN_REFERENCES: AgentComposerPlanReference[] = [];

const surfaceStyle = {
  backgroundColor: "var(--bg-surface)",
  borderColor: "var(--border-subtle)",
  borderWidth: 1,
  borderStyle: "solid",
} as const;

const elevatedSurfaceStyle = {
  backgroundColor: "var(--bg-elevated)",
  borderColor: "var(--overlay-faint)",
  borderWidth: 1,
  borderStyle: "solid",
} as const;

function titleFromMarkdownFile(fileName: string): string {
  const withoutExtension = fileName.replace(/\.(markdown|md)$/i, "");
  return withoutExtension.replace(/[_-]+/g, " ").trim() || "Imported plan";
}

function formatErrorMessage(error: unknown, fallback: string): string {
  return error instanceof Error && error.message.trim()
    ? error.message
    : fallback;
}

export function AgentPlanStartPanel({
  status = "idle",
  errorMessage = null,
  projectId = null,
  conversationId = null,
  onDraftCreated,
}: AgentPlanStartPanelProps) {
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedPlan, setSelectedPlan] =
    useState<AgentComposerPlanReference | null>(null);
  const [selectedVersion, setSelectedVersion] = useState<number | null>(null);
  const [operationError, setOperationError] = useState<string | null>(null);
  const [isCopyPending, setIsCopyPending] = useState(false);
  const [isImportPending, setIsImportPending] = useState(false);
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const statusCopy = STATUS_COPY[status];
  const canSearch = Boolean(projectId) && status === "idle";
  const canMutate = Boolean(conversationId) && status === "idle";
  const isActionPending = isCopyPending || isImportPending;

  const planReferencesQuery = useAgentComposerPlanReferences({
    projectId: projectId ?? "",
    query: searchQuery,
    enabled: canSearch,
  });
  const plans = planReferencesQuery.data?.plans ?? EMPTY_PLAN_REFERENCES;

  useEffect(() => {
    setSelectedVersion(selectedPlan?.artifactVersion ?? null);
    setOperationError(null);
  }, [
    selectedPlan?.artifactId,
    selectedPlan?.artifactVersion,
    selectedPlan?.sessionId,
  ]);

  const selectedPlanIsVisible = useMemo(() => {
    if (!selectedPlan) {
      return true;
    }
    if (planReferencesQuery.isFetching) {
      return true;
    }
    return plans.some((plan) => samePlanReference(plan, selectedPlan));
  }, [planReferencesQuery.isFetching, plans, selectedPlan]);

  const versionHistoryQuery = useQuery({
    queryKey: [
      "agents",
      "plan-start",
      "version-history",
      selectedPlan?.artifactId ?? null,
    ],
    queryFn: () => artifactApi.getVersionHistory(selectedPlan!.artifactId),
    enabled: Boolean(selectedPlan?.artifactId),
    staleTime: 30_000,
  });

  const versionOptions = useMemo(() => {
    if (!selectedPlan) {
      return [];
    }
    const versions = new Map<number, string | null>();
    versions.set(selectedPlan.artifactVersion, null);
    for (const version of versionHistoryQuery.data ?? []) {
      versions.set(version.version, version.created_at);
    }
    return Array.from(versions.entries())
      .sort(([a], [b]) => b - a)
      .map(([version, createdAt]) => ({ version, createdAt }));
  }, [selectedPlan, versionHistoryQuery.data]);

  const previewQuery = useQuery({
    queryKey: [
      "agents",
      "plan-start",
      "preview",
      selectedPlan?.artifactId ?? null,
      selectedVersion,
    ],
    queryFn: () =>
      artifactApi.getAtVersion(selectedPlan!.artifactId, selectedVersion!),
    enabled: Boolean(selectedPlan?.artifactId && selectedVersion),
    staleTime: 15_000,
  });

  const handleDraftCreated = useCallback(
    async (draft: AgentConversationPlanDraft) => {
      setOperationError(null);
      await onDraftCreated?.(draft);
    },
    [onDraftCreated],
  );

  const handleCopyPlan = useCallback(async () => {
    if (
      !conversationId ||
      !selectedPlan ||
      !selectedVersion ||
      !selectedPlanIsVisible ||
      isActionPending
    ) {
      return;
    }

    let draft: AgentConversationPlanDraft | null = null;
    const selectedTitle = planTitle(selectedPlan);
    const confirmed = await confirm({
      title: "Copy plan?",
      description: `Copy "${selectedTitle}" v${selectedVersion} into this conversation as a draft plan.`,
      confirmText: "Copy plan",
      pendingText: "Copying...",
      onConfirm: async () => {
        setIsCopyPending(true);
        try {
          draft = await chatApi.copyAgentConversationPlan({
            conversationId,
            sourceSessionId: selectedPlan.sessionId,
            sourceArtifactId: selectedPlan.artifactId,
            sourceVersion: selectedVersion,
          });
        } catch (error) {
          setOperationError(formatErrorMessage(error, "Failed to copy plan"));
          throw error;
        } finally {
          setIsCopyPending(false);
        }
      },
    });

    if (confirmed && draft) {
      try {
        await handleDraftCreated(draft);
      } catch (error) {
        setOperationError(
          formatErrorMessage(error, "Plan copied, but the workspace did not refresh."),
        );
      }
    }
  }, [
    confirm,
    conversationId,
    handleDraftCreated,
    isActionPending,
    selectedPlan,
    selectedPlanIsVisible,
    selectedVersion,
  ]);

  const handleMarkdownDrop = useCallback(
    async (file: File, content: string) => {
      if (!conversationId || isActionPending) {
        setOperationError("Open an agent conversation before importing a plan.");
        return;
      }
      setIsImportPending(true);
      try {
        const draft = await chatApi.importAgentConversationPlanMarkdown({
          conversationId,
          title: titleFromMarkdownFile(file.name),
          content,
        });
        await handleDraftCreated(draft);
      } catch (error) {
        setOperationError(formatErrorMessage(error, "Failed to import markdown plan"));
      } finally {
        setIsImportPending(false);
      }
    },
    [conversationId, handleDraftCreated, isActionPending],
  );

  const {
    isDragging,
    dropProps,
    error: fileDropError,
  } = useFileDrop({
    acceptedExtensions: [".md"],
    maxSizeBytes: PLAN_IMPORT_MAX_BYTES,
    onFileDrop: handleMarkdownDrop,
    onError: (error) => setOperationError(error.message),
    enabled: canMutate && !isActionPending,
  });

  const statusDetail =
    status === "error" && errorMessage?.trim()
      ? errorMessage.trim()
      : statusCopy.detail;
  const selectedPreview =
    previewQuery.data?.content.type === "inline"
      ? previewQuery.data.content.text
      : null;
  const staleSelectionMessage =
    selectedPlan && !selectedPlanIsVisible
      ? "Selected plan is no longer in the current results. Select it again before copying."
      : null;
  const visibleError =
    operationError ?? fileDropError?.message ?? staleSelectionMessage ?? null;
  const copyDisabled =
    !conversationId ||
    !selectedPlan ||
    !selectedVersion ||
    !selectedPlanIsVisible ||
    previewQuery.isError ||
    isActionPending;

  return (
    <div
      className="min-h-full px-4 py-4"
      data-testid="agent-plan-start-panel"
    >
      <div className="mx-auto flex max-w-5xl flex-col gap-4">
        <section
          className="rounded-lg px-4 py-4"
          style={surfaceStyle}
          aria-labelledby="agent-plan-start-heading"
        >
          <div className="flex items-start gap-3">
            <div
              className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md"
              style={{
                backgroundColor: "var(--accent-muted)",
                color: "var(--accent-primary)",
              }}
            >
              <FileText className="h-4 w-4" aria-hidden="true" />
            </div>
            <div className="min-w-0">
              <h2
                id="agent-plan-start-heading"
                className="text-sm font-semibold"
                style={{ color: "var(--text-primary)" }}
              >
                Start a Plan
              </h2>
              <p
                className="mt-1 max-w-2xl text-sm leading-5"
                style={{ color: "var(--text-secondary)" }}
              >
                Create a draft from an existing project plan or a markdown file.
              </p>
            </div>
          </div>
        </section>

        <div className="grid gap-4 xl:grid-cols-[minmax(260px,0.88fr)_minmax(320px,1.12fr)]">
          <section className="rounded-lg p-4" style={surfaceStyle}>
            <label
              htmlFor="agent-plan-start-search"
              className="text-xs font-medium uppercase tracking-[0.08em]"
              style={{ color: "var(--text-muted)" }}
            >
              Project plans
            </label>
            <div
              className="mt-2 flex h-10 items-center gap-2 rounded-md px-3"
              style={elevatedSurfaceStyle}
            >
              <Search
                className="h-4 w-4 shrink-0"
                style={{ color: "var(--text-muted)" }}
                aria-hidden="true"
              />
              <input
                id="agent-plan-start-search"
                type="search"
                aria-label="Search project plans"
                disabled={!canSearch || isActionPending}
                className="min-w-0 flex-1 bg-transparent text-sm outline-none"
                placeholder="Search existing plans"
                style={{ color: "var(--text-primary)" }}
                value={searchQuery}
                onChange={(event) => setSearchQuery(event.target.value)}
              />
            </div>

            <AgentPlanStartPanelSearch
              plans={plans}
              status={status}
              isLoading={
                planReferencesQuery.isLoading ||
                (planReferencesQuery.isFetching && plans.length === 0)
              }
              isError={planReferencesQuery.isError}
              selectedPlan={selectedPlan}
              onSelectPlan={setSelectedPlan}
            />

            <div
              className="mt-3 rounded-md px-3 py-3 text-sm"
              style={elevatedSurfaceStyle}
              role={status === "error" ? "alert" : "status"}
              aria-live="polite"
              data-testid={`agent-plan-start-status-${status}`}
            >
              <div className="font-medium" style={{ color: "var(--text-primary)" }}>
                {statusCopy.label}
              </div>
              <div className="mt-1 leading-5">{statusDetail}</div>
            </div>
          </section>

          <section className="flex min-h-[360px] flex-col gap-4 rounded-lg p-4" style={surfaceStyle}>
            <div>
              <div className="flex items-center gap-2">
                <Upload
                  className="h-4 w-4"
                  style={{ color: "var(--accent-primary)" }}
                  aria-hidden="true"
                />
                <h3
                  className="text-sm font-semibold"
                  style={{ color: "var(--text-primary)" }}
                >
                  Import markdown
                </h3>
              </div>
              <div
                {...dropProps}
                className={cn(
                  "relative mt-3 overflow-hidden rounded-md px-3 py-5 text-center text-sm",
                  !canMutate || isActionPending ? "opacity-70" : "",
                )}
                style={{
                  backgroundColor: "var(--bg-elevated)",
                  color: "var(--text-secondary)",
                  borderColor: isDragging
                    ? "var(--accent-primary)"
                    : "var(--overlay-faint)",
                  borderWidth: 1,
                  borderStyle: "dashed",
                }}
                aria-disabled={!canMutate || isActionPending}
              >
                <DropZoneOverlay isVisible={isDragging} message="Drop to import plan" />
                {isImportPending ? "Importing markdown..." : "Markdown drop area"}
              </div>
            </div>

            <AgentPlanStartPanelPreview
              selectedPlan={selectedPlan}
              selectedVersion={selectedVersion}
              versionOptions={versionOptions}
              isLoading={previewQuery.isFetching}
              isError={previewQuery.isError}
              preview={selectedPreview}
              onVersionChange={setSelectedVersion}
            />

            {visibleError && (
              <div
                className="flex items-start gap-2 rounded-md px-3 py-2 text-sm"
                style={{
                  backgroundColor: "var(--status-error-muted)",
                  borderColor: "var(--status-error-border)",
                  borderWidth: 1,
                  borderStyle: "solid",
                  color: "var(--text-primary)",
                }}
                role="alert"
              >
                <AlertCircle
                  className="mt-0.5 h-4 w-4 shrink-0"
                  style={{ color: "var(--status-error)" }}
                  aria-hidden="true"
                />
                <span>{visibleError}</span>
              </div>
            )}

            <div className="mt-auto flex justify-end">
              <Button
                type="button"
                onClick={handleCopyPlan}
                disabled={copyDisabled}
                className="gap-2"
              >
                {isCopyPending && (
                  <Loader2 aria-hidden="true" className="h-4 w-4 animate-spin" />
                )}
                Copy plan
              </Button>
            </div>
          </section>
        </div>
      </div>
      <ConfirmationDialog {...confirmationDialogProps} />
    </div>
  );
}
