import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";

import { artifactApi } from "@/api/artifact";
import {
  ArtifactLoadingState,
  EmptyArtifactState,
} from "@/components/agents/AgentsArtifactEmptyState";
import { VersionedArtifactDisplay } from "@/components/Ideation/PlanDisplay";
import { PersonaContentDiff } from "@/components/personas/PersonaContentDiff";
import { preparePersonaArtifactContent } from "@/components/personas/personaArtifactContent";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import { useAgentGate } from "@/hooks/useAgentGate";
import { personaArtifactKeys } from "@/hooks/personaArtifactQueries";
import { usePersonaDraftEvents } from "@/hooks/usePersonaDraftEvents";
import {
  useApprovePersona,
  useApprovePersonaAsNew,
  usePersona,
  useReseedPersonaDraft,
} from "@/hooks/usePersonas";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";
import type { Artifact } from "@/types/artifact";
import type { ChatConversation } from "@/types/chat-conversation";
import type { Persona } from "@/types/persona";

interface PersonaArtifactPanelProps {
  conversation: ChatConversation;
}

/** Stable IPC prefix for a seeded approval whose source changed after seeding. */
const SOURCE_CHANGED_SINCE_SEED_PREFIX = "SourceChangedSinceSeed:";

function inlineArtifactText(artifact: Artifact): string {
  return artifact.content.type === "inline" ? artifact.content.text : "";
}

export function PersonaArtifactSkeleton() {
  return <ArtifactLoadingState title="Loading persona..." />;
}

export function PersonaArtifactPanel({ conversation }: PersonaArtifactPanelProps) {
  const eventDraftId = usePersonaDraftEvents(conversation.id);
  const [approvedPersona, setApprovedPersona] = useState<Persona | null>(null);
  const draftId = conversation.builderDraftId ?? eventDraftId;
  const resultId = conversation.builderResultPersonaId ?? null;
  const boundPersonaId = approvedPersona?.id ?? draftId ?? resultId;
  const personaQuery = usePersona(boundPersonaId ?? "");
  const persona = approvedPersona ?? personaQuery.data ?? null;
  const artifactId = persona?.artifactId ?? "";
  const artifactQuery = useQuery({
    queryKey: personaArtifactKeys.detail(artifactId),
    queryFn: () => artifactApi.get(artifactId),
    enabled: Boolean(artifactId),
    staleTime: 30_000,
  });
  const { data: featureFlags } = useFeatureFlags();
  const approve = useApprovePersona();
  const approveAsNew = useApprovePersonaAsNew();
  const reseedDraft = useReseedPersonaDraft();
  const approveGate = useAgentGate("personaApprove");
  const approveAsNewGate = useAgentGate("personaApproveAsNew");
  const reseedGate = useAgentGate("personaReseedDraft");
  const [showChanges, setShowChanges] = useState(false);
  const [approvalConflictRevealed, setApprovalConflictRevealed] = useState(false);
  const sourcePersonaQuery = usePersona(persona?.sourcePersonaId ?? "");
  const versionHistoryQuery = useQuery({
    queryKey: [...personaArtifactKeys.detail(artifactId), "versions"],
    queryFn: () => artifactApi.getVersionHistory(artifactId),
    enabled: Boolean(artifactId) && showChanges,
    staleTime: 30_000,
  });
  const previousVersionId = versionHistoryQuery.data?.[1]?.id ?? null;
  const previousArtifactQuery = useQuery({
    queryKey: personaArtifactKeys.detail(previousVersionId ?? ""),
    queryFn: () => artifactApi.get(previousVersionId ?? ""),
    enabled: Boolean(previousVersionId) && showChanges,
    staleTime: 30_000,
  });
  const openModal = useUiStore((state) => state.openModal);
  const setCurrentView = useUiStore((state) => state.setCurrentView);
  const setStartConversationDraft = useAgentSessionStore(
    (state) => state.setStartConversationDraft,
  );
  const setFocusedProject = useAgentSessionStore((state) => state.setFocusedProject);
  const clearSelection = useAgentSessionStore((state) => state.clearSelection);

  useEffect(() => {
    setApprovedPersona(null);
    setShowChanges(false);
    setApprovalConflictRevealed(false);
  }, [conversation.id]);

  const isDraft = persona?.status === "draft" && Boolean(draftId) && !approvedPersona;
  const mutationError = approve.error ?? approveAsNew.error;
  const isMutating = approve.isPending || approveAsNew.isPending;
  const seededDraft = Boolean(persona?.sourcePersonaId);
  const isPersonaBuilderConversation = conversation.agentMode === "persona_builder";

  if (!boundPersonaId) {
    return (
      <EmptyArtifactState
        title="Persona not created yet"
        detail="The agent will draft the persona here after its first pass"
      />
    );
  }

  if (personaQuery.isPending && !approvedPersona) {
    return <PersonaArtifactSkeleton />;
  }

  if (personaQuery.error || !persona) {
    return (
      <div className="min-h-full px-4 pb-4 pt-4">
        <div role="alert" className="rounded-md border border-[var(--status-error-border)] p-3 text-sm text-[var(--status-error)]">
          {personaQuery.error?.message ?? "Persona unavailable"}
        </div>
      </div>
    );
  }

  if (persona.artifactId && artifactQuery.isPending) {
    return <PersonaArtifactSkeleton />;
  }

  if (persona.artifactId && (artifactQuery.error || !artifactQuery.data)) {
    return (
      <div className="min-h-full px-4 pb-4 pt-4">
        <div role="alert" className="rounded-md border border-[var(--status-error-border)] p-3 text-sm text-[var(--status-error)]">
          {artifactQuery.error?.message ?? "Persona artifact unavailable"}
        </div>
      </div>
    );
  }

  const artifact: Artifact = artifactQuery.data ?? {
    id: persona.id,
    type: "persona",
    name: persona.name,
    content: { type: "inline", text: persona.content },
    metadata: {
      createdAt: persona.createdAt,
      createdBy: "system",
      version: 1,
    },
    derivedFrom: [],
  };

  const approveDraft = async (asNew: boolean) => {
    const gate = asNew ? approveAsNewGate : approveGate;
    if (gate.gated) return;
    try {
      const approved = asNew
        ? await approveAsNew.mutateAsync({ id: persona.id })
        : await approve.mutateAsync(persona.id);
      setApprovedPersona(approved);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (message.includes(SOURCE_CHANGED_SINCE_SEED_PREFIX)) {
        setApprovalConflictRevealed(true);
      }
    }
  };

  const rebaseDraft = async () => {
    if (reseedGate.gated) return;
    try {
      await reseedDraft.mutateAsync(persona.id);
      setApprovalConflictRevealed(false);
    } catch {
      // reseedDraft.error renders below.
    }
  };

  const sourceStale =
    seededDraft &&
    isDraft &&
    ((persona.sourceContentHash != null &&
      sourcePersonaQuery.data != null &&
      sourcePersonaQuery.data.contentHash !== persona.sourceContentHash) ||
      approvalConflictRevealed);

  const refinePersona = () => {
    setStartConversationDraft({
      projectId: persona.projectId,
      projectLocked: true,
      mode: "persona_builder",
      sourcePersonaId: persona.id,
      sourcePersonaName: persona.name,
    });
    setFocusedProject(persona.projectId);
    clearSelection();
    setCurrentView("agents");
  };
  const globalRefineDisabled =
    persona.projectId === null && !featureFlags.standaloneConversations;
  const refineButton = (
    <Button
      type="button"
      size="sm"
      disabled={persona.status === "archived" || globalRefineDisabled}
      onClick={refinePersona}
    >
      Refine with Agent
    </Button>
  );

  const artifactActions = isDraft ? (
    seededDraft ? (
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={isMutating}
            aria-disabled={approveAsNewGate.gated || undefined}
            data-disabled-explained={approveAsNewGate.gated ? "true" : undefined}
            onClick={() => void approveDraft(true)}
            className={approveAsNewGate.gated ? "opacity-50" : undefined}
          >
            Approve as new
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {approveAsNewGate.gated ? approveAsNewGate.reason : "Approve as new"}
        </TooltipContent>
      </Tooltip>
    ) : null
  ) : (
    <>
      <Button
        type="button"
        size="sm"
        variant="outline"
        onClick={() =>
          openModal("settings", {
            section: "personas",
            personaId: persona.id,
            conversationId: conversation.id,
          })
        }
      >
        Open in Settings
      </Button>
      {!isPersonaBuilderConversation &&
        (globalRefineDisabled ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="inline-flex" tabIndex={0}>
                {refineButton}
              </span>
            </TooltipTrigger>
            <TooltipContent>
              Global persona refinement requires standalone conversations
            </TooltipContent>
          </Tooltip>
        ) : (
          refineButton
        ))}
    </>
  );

  const canShowChanges = Boolean(persona.artifactId) && artifact.metadata.version > 1;
  const showChangesToggle = canShowChanges && (
    <div className="mb-2 flex justify-end">
      <Button
        type="button"
        variant="outline"
        size="sm"
        aria-pressed={showChanges}
        data-testid="persona-show-changes-toggle"
        onClick={() => setShowChanges((current) => !current)}
      >
        {showChanges ? "Hide changes" : "Show changes"}
      </Button>
    </div>
  );

  return (
    <div className="min-h-full px-4 pb-4 pt-4">
      {sourceStale && (
        <div
          role="alert"
          data-testid="persona-stale-source-banner"
          className="mb-3 flex flex-wrap items-center justify-between gap-2 rounded-md border border-[var(--status-warning-border)] bg-[var(--status-warning-muted)] px-3 py-2 text-sm text-[var(--text-primary)]"
        >
          <span>Source persona changed since this draft was seeded.</span>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={reseedDraft.isPending}
                aria-disabled={reseedGate.gated || undefined}
                data-disabled-explained={reseedGate.gated ? "true" : undefined}
                onClick={() => void rebaseDraft()}
                className={reseedGate.gated ? "opacity-50" : undefined}
              >
                {reseedDraft.isPending
                  ? "Rebasing..."
                  : "Rebase draft on current source"}
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {reseedGate.gated ? reseedGate.reason : "Rebase draft on current source"}
            </TooltipContent>
          </Tooltip>
        </div>
      )}
      {showChangesToggle}
      {showChanges && canShowChanges ? (
        versionHistoryQuery.isError || previousArtifactQuery.isError ? (
          <div
            role="alert"
            className="rounded-md border border-[var(--status-error-border)] p-3 text-sm text-[var(--status-error)]"
          >
            Could not load the previous version to compare.
          </div>
        ) : !previousVersionId && versionHistoryQuery.isSuccess ? (
          <p className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2 text-xs text-[var(--text-muted)]">
            This is the first version — nothing to compare.
          </p>
        ) : previousArtifactQuery.data ? (
          <PersonaContentDiff
            oldContent={inlineArtifactText(previousArtifactQuery.data)}
            newContent={inlineArtifactText(artifact)}
            ariaLabel="Changes since the previous version"
          />
        ) : (
          <div className="min-h-24 animate-pulse rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]" />
        )
      ) : (
        <VersionedArtifactDisplay
          artifact={artifact}
          artifactLabel="Persona"
          showApprove={false}
          onApprove={() => void approveDraft(false)}
          isApproving={isMutating}
          approveLabel="Approve Persona"
          artifactActions={
            isDraft ? (
              <>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      type="button"
                      size="sm"
                      disabled={isMutating}
                      aria-disabled={approveGate.gated || undefined}
                      data-disabled-explained={approveGate.gated ? "true" : undefined}
                      onClick={() => void approveDraft(false)}
                      className={approveGate.gated ? "opacity-50" : undefined}
                    >
                      Approve Persona
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>
                    {approveGate.gated ? approveGate.reason : "Approve Persona"}
                  </TooltipContent>
                </Tooltip>
                {artifactActions}
              </>
            ) : (
              artifactActions
            )
          }
          excerptSelectionEnabled={false}
          prepareContent={preparePersonaArtifactContent}
          linkedProposalsCount={0}
          chromeless
        />
      )}
      {(mutationError ?? reseedDraft.error) && (
        <div role="alert" className="mt-4 text-sm text-[var(--status-error)]">
          {(mutationError ?? reseedDraft.error)?.message}
        </div>
      )}
    </div>
  );
}
