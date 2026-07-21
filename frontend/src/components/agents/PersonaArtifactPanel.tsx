import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";

import { artifactApi } from "@/api/artifact";
import {
  ArtifactLoadingState,
  EmptyArtifactState,
} from "@/components/agents/AgentsArtifactEmptyState";
import { VersionedArtifactDisplay } from "@/components/Ideation/PlanDisplay";
import { preparePersonaArtifactContent } from "@/components/personas/personaArtifactContent";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import { personaArtifactKeys } from "@/hooks/personaArtifactQueries";
import { usePersonaDraftEvents } from "@/hooks/usePersonaDraftEvents";
import {
  useApprovePersona,
  useApprovePersonaAsNew,
  usePersona,
} from "@/hooks/usePersonas";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";
import type { Artifact } from "@/types/artifact";
import type { ChatConversation } from "@/types/chat-conversation";
import type { Persona } from "@/types/persona";

interface PersonaArtifactPanelProps {
  conversation: ChatConversation;
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
  const openModal = useUiStore((state) => state.openModal);
  const setCurrentView = useUiStore((state) => state.setCurrentView);
  const setStartConversationDraft = useAgentSessionStore(
    (state) => state.setStartConversationDraft,
  );
  const setFocusedProject = useAgentSessionStore((state) => state.setFocusedProject);
  const clearSelection = useAgentSessionStore((state) => state.clearSelection);

  useEffect(() => {
    setApprovedPersona(null);
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
    const approved = asNew
      ? await approveAsNew.mutateAsync({ id: persona.id })
      : await approve.mutateAsync(persona.id);
    setApprovedPersona(approved);
  };

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
      <Button
        type="button"
        size="sm"
        variant="outline"
        disabled={isMutating}
        onClick={() => void approveDraft(true)}
      >
        Approve as new
      </Button>
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

  return (
    <div className="min-h-full px-4 pb-4 pt-4">
      <VersionedArtifactDisplay
        artifact={artifact}
        artifactLabel="Persona"
        showApprove={isDraft}
        onApprove={() => void approveDraft(false)}
        isApproving={isMutating}
        approveLabel="Approve Persona"
        artifactActions={artifactActions}
        excerptSelectionEnabled={false}
        prepareContent={preparePersonaArtifactContent}
        linkedProposalsCount={0}
        chromeless
      />
      {mutationError && (
        <div role="alert" className="mt-4 text-sm text-[var(--status-error)]">
          {mutationError.message}
        </div>
      )}
    </div>
  );
}
