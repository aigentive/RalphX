import { useMutation, useQuery } from "@tanstack/react-query";
import { FileText, Upload } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";

import { artifactApi } from "@/api/artifact";
import {
  chatApi,
  type AgentConversationPlanSeedResult,
} from "@/api/chat";
import type { AgentComposerPlanReference } from "@/api/agent-composer";
import { Button } from "@/components/ui/button";
import { useAgentComposerPlanReferences } from "@/hooks/useAgentComposerResources";
import { useFileDrop, type FileDropError } from "@/hooks/useFileDrop";
import { cn } from "@/lib/utils";
import type { Artifact } from "@/types/artifact";
import type { ArtifactVersionSummary } from "@/types/artifact";

interface AgentPlanStartPanelProps {
  conversationId: string;
  projectId: string;
  onPlanSeeded: (result: AgentConversationPlanSeedResult) => void;
}

export function AgentPlanStartPanel({
  conversationId,
  projectId,
  onPlanSeeded,
}: AgentPlanStartPanelProps) {
  const [query, setQuery] = useState("");
  const [hasSearchIntent, setHasSearchIntent] = useState(false);
  const [selectedPlan, setSelectedPlan] =
    useState<AgentComposerPlanReference | null>(null);
  const [selectedVersion, setSelectedVersion] = useState<number | null>(null);

  const planReferencesQuery = useAgentComposerPlanReferences({
    projectId,
    query,
    enabled: hasSearchIntent,
  });

  useEffect(() => {
    if (!selectedPlan) {
      setSelectedVersion(null);
      return;
    }
    setSelectedVersion(selectedPlan.artifactVersion);
  }, [selectedPlan]);

  const versionHistoryQuery = useQuery({
    queryKey: [
      "agents",
      "plan-start",
      "versions",
      selectedPlan?.artifactId ?? null,
    ],
    queryFn: () => artifactApi.getVersionHistory(selectedPlan!.artifactId),
    enabled: Boolean(selectedPlan),
    staleTime: 30_000,
  });

  const versionSummaries = useMemo<ArtifactVersionSummary[]>(() => {
    if (versionHistoryQuery.data?.length) {
      return versionHistoryQuery.data;
    }
    if (!selectedPlan) {
      return [];
    }
    return [
      {
        id: selectedPlan.artifactId,
        version: selectedPlan.artifactVersion,
        name: selectedPlan.title ?? "Plan",
        created_at: selectedPlan.updatedAt,
        created_by: "system",
        metadata: null,
      },
    ];
  }, [selectedPlan, versionHistoryQuery.data]);

  const previewQuery = useQuery({
    queryKey: [
      "agents",
      "plan-start",
      "preview",
      selectedPlan?.artifactId ?? null,
      selectedVersion,
    ],
    queryFn: () => artifactApi.getAtVersion(selectedPlan!.artifactId, selectedVersion!),
    enabled: Boolean(selectedPlan && selectedVersion),
    staleTime: 10_000,
  });

  const copyMutation = useMutation({
    mutationFn: async () => {
      if (!selectedPlan || !selectedVersion) {
        throw new Error("Select a plan first");
      }
      return chatApi.copyAgentConversationPlan({
        conversationId,
        sourceSessionId: selectedPlan.sessionId,
        sourceArtifactId: selectedPlan.artifactId,
        sourceVersion: selectedVersion,
      });
    },
    onSuccess: (result) => {
      onPlanSeeded(result);
      toast.success("Plan copied");
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : "Failed to copy plan");
    },
  });

  const importMutation = useMutation({
    mutationFn: ({
      title,
      content,
    }: {
      title: string;
      content: string;
    }) =>
      chatApi.importAgentConversationPlan({
        conversationId,
        title,
        content,
      }),
    onSuccess: (result) => {
      onPlanSeeded(result);
      toast.success("Plan imported");
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : "Failed to import plan");
    },
  });

  const handleFileDrop = useCallback(
    (file: File, content: string) => {
      const title = file.name
        .replace(/\.md$/i, "")
        .replace(/_/g, " ")
        .trim();
      importMutation.mutate({
        title: title || "Imported plan",
        content,
      });
    },
    [importMutation],
  );

  const handleFileDropError = useCallback((error: FileDropError) => {
    toast.error(error.message);
  }, []);

  const { isDragging, dropProps, error: dropError } = useFileDrop({
    acceptedExtensions: [".md"],
    onFileDrop: handleFileDrop,
    onError: handleFileDropError,
    enabled: Boolean(conversationId && projectId),
  });

  const previewArtifact = previewQuery.data ?? null;
  const previewText = planPreviewText(previewArtifact);
  const copyDisabled =
    copyMutation.isPending ||
    !selectedPlan ||
    !selectedVersion ||
    previewQuery.isLoading;

  return (
    <div
      className="flex h-full min-h-0 flex-col gap-4 p-4"
      data-testid="agent-plan-start-panel"
    >
      <div className="flex items-center gap-2">
        <FileText className="h-4 w-4" style={{ color: "var(--accent-primary)" }} />
        <h2
          className="text-sm font-semibold"
          style={{ color: "var(--text-primary)" }}
        >
          Plan
        </h2>
      </div>

      <div
        {...dropProps}
        data-testid="agent-plan-drop-zone"
        className={cn(
          "rounded-md border p-3 transition-colors",
          isDragging ? "border-dashed" : "",
        )}
        style={{
          borderColor: isDragging ? "var(--accent-primary)" : "var(--border-subtle)",
          background: isDragging ? "var(--accent-muted)" : "var(--bg-base)",
        }}
      >
        <div className="flex items-center gap-2">
          <Upload className="h-4 w-4" style={{ color: "var(--text-muted)" }} />
          <span className="text-xs font-medium" style={{ color: "var(--text-primary)" }}>
            Drop Markdown plan
          </span>
        </div>
        {dropError && (
          <p className="mt-2 text-xs" style={{ color: "var(--status-error)" }}>
            {dropError.message}
          </p>
        )}
      </div>

      <div className="space-y-2">
        <label
          className="block text-xs font-medium"
          htmlFor="agent-plan-start-search"
          style={{ color: "var(--text-muted)" }}
        >
          Search project plans
        </label>
        <input
          id="agent-plan-start-search"
          aria-label="Search project plans"
          value={query}
          onFocus={() => setHasSearchIntent(true)}
          onChange={(event) => {
            setHasSearchIntent(true);
            setQuery(event.target.value);
          }}
          className="h-9 w-full rounded-md border px-3 text-sm outline-none"
          style={{
            borderColor: "var(--border-subtle)",
            background: "var(--bg-surface)",
            color: "var(--text-primary)",
          }}
        />
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {planReferencesQuery.isError ? (
          <p className="text-xs" style={{ color: "var(--status-error)" }}>
            Failed to load plans
          </p>
        ) : null}
        <div className="space-y-2">
          {(planReferencesQuery.data?.plans ?? []).map((plan) => (
            <button
              key={`${plan.sessionId}:${plan.artifactId}`}
              type="button"
              onClick={() => setSelectedPlan(plan)}
              className="w-full rounded-md border p-3 text-left transition-colors"
              style={{
                borderColor:
                  selectedPlan?.artifactId === plan.artifactId
                    ? "var(--accent-primary)"
                    : "var(--border-subtle)",
                background:
                  selectedPlan?.artifactId === plan.artifactId
                    ? "var(--accent-muted)"
                    : "var(--bg-surface)",
                color: "var(--text-primary)",
              }}
            >
              <div className="flex items-center justify-between gap-3">
                <span className="min-w-0 truncate text-sm font-medium">
                  {plan.title ?? "Untitled plan"}
                </span>
                <span className="shrink-0 text-xs" style={{ color: "var(--text-muted)" }}>
                  v{plan.artifactVersion}
                </span>
              </div>
              <div className="mt-1 text-xs capitalize" style={{ color: "var(--text-muted)" }}>
                {plan.status}
              </div>
            </button>
          ))}
        </div>
      </div>

      {selectedPlan && (
        <div className="space-y-3 border-t pt-3" style={{ borderColor: "var(--border-subtle)" }}>
          <div className="flex items-center gap-2">
            <label
              className="text-xs font-medium"
              htmlFor="agent-plan-start-version"
              style={{ color: "var(--text-muted)" }}
            >
              Plan version
            </label>
            <select
              id="agent-plan-start-version"
              aria-label="Plan version"
              value={selectedVersion ?? ""}
              onChange={(event) => setSelectedVersion(Number(event.target.value))}
              className="h-8 rounded-md border px-2 text-sm outline-none"
              style={{
                borderColor: "var(--border-subtle)",
                background: "var(--bg-surface)",
                color: "var(--text-primary)",
              }}
            >
              {versionSummaries.map((version) => (
                <option key={version.id} value={version.version}>
                  v{version.version}
                </option>
              ))}
            </select>
            <Button
              type="button"
              size="sm"
              onClick={() => copyMutation.mutate()}
              disabled={copyDisabled}
              className="ml-auto h-8"
            >
              Copy plan
            </Button>
          </div>

          <div
            className="max-h-52 overflow-auto rounded-md border p-3 text-xs"
            style={{
              borderColor: "var(--border-subtle)",
              background: "var(--bg-base)",
              color: "var(--text-primary)",
            }}
          >
            {previewQuery.isLoading ? (
              <span style={{ color: "var(--text-muted)" }}>Loading preview...</span>
            ) : previewText ? (
              <pre className="whitespace-pre-wrap font-mono">{previewText}</pre>
            ) : (
              <span style={{ color: "var(--text-muted)" }}>No preview available</span>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function planPreviewText(artifact: Artifact | null): string | null {
  if (!artifact) {
    return null;
  }
  if (artifact.content.type !== "inline") {
    return "File-backed preview unavailable";
  }
  return artifact.content.text;
}
