import { useEffect, useState, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { PersonaVersionHistory } from "@/components/personas/PersonaVersionHistory";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import { usePersonaDraftEvents } from "@/hooks/usePersonaDraftEvents";
import {
  useApprovePersona,
  useApprovePersonaAsNew,
  usePersona,
} from "@/hooks/usePersonas";
import { cn } from "@/lib/utils";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";
import type { ChatConversation } from "@/types/chat-conversation";
import type { Persona } from "@/types/persona";

interface PersonaArtifactPanelProps {
  conversation: ChatConversation;
}

function PersonaPanelShell({ children }: { children: ReactNode }) {
  return (
    <section className="flex h-full min-h-0 flex-col" aria-labelledby="persona-artifact-heading">
      <div className="shrink-0 border-b border-[var(--border-subtle)] px-5 py-4">
        <h2 id="persona-artifact-heading" className="text-sm font-semibold text-[var(--text-primary)]">
          Persona
        </h2>
      </div>
      {children}
    </section>
  );
}

export function PersonaArtifactSkeleton() {
  return (
    <PersonaPanelShell>
      <div className="space-y-4 p-5" aria-label="Loading persona">
        <div className="h-5 w-40 animate-pulse rounded bg-[var(--bg-elevated)]" />
        <div className="h-8 w-full animate-pulse rounded bg-[var(--bg-elevated)]" />
        <div className="space-y-2 pt-2">
          <div className="h-4 w-3/4 animate-pulse rounded bg-[var(--bg-elevated)]" />
          <div className="h-4 w-full animate-pulse rounded bg-[var(--bg-elevated)]" />
          <div className="h-4 w-5/6 animate-pulse rounded bg-[var(--bg-elevated)]" />
        </div>
      </div>
    </PersonaPanelShell>
  );
}

function StatusPill({ status }: { status: Persona["status"] }) {
  const label = status === "active" ? "Approved" : status === "archived" ? "Archived" : "Draft";
  return (
    <span
      className={cn(
        "rounded-full border px-2 py-0.5 text-[11px] font-medium",
        status === "active" && "border-[var(--status-success-border)] text-[var(--status-success)]",
        status === "archived" && "border-[var(--border-default)] text-[var(--text-muted)]",
        status === "draft" && "border-[var(--accent-primary)] text-[var(--accent-primary)]",
      )}
    >
      {label}
    </span>
  );
}

export function PersonaArtifactPanel({ conversation }: PersonaArtifactPanelProps) {
  const eventDraftId = usePersonaDraftEvents(conversation.id);
  const [approvedPersona, setApprovedPersona] = useState<Persona | null>(null);
  const [selectedArtifactVersion, setSelectedArtifactVersion] = useState<number | null>(null);
  const draftId = conversation.builderDraftId ?? eventDraftId;
  const resultId = conversation.builderResultPersonaId ?? null;
  const boundPersonaId = approvedPersona?.id ?? draftId ?? resultId;
  const personaQuery = usePersona(boundPersonaId ?? "");
  const persona = approvedPersona ?? personaQuery.data ?? null;
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
    setSelectedArtifactVersion(null);
  }, [conversation.id]);

  const isHistorical = selectedArtifactVersion != null;
  const isDraft = persona?.status === "draft" && Boolean(draftId) && !approvedPersona;
  const mutationError = approve.error ?? approveAsNew.error;
  const isMutating = approve.isPending || approveAsNew.isPending;
  const seededDraft = Boolean(persona?.sourcePersonaId);

  if (!boundPersonaId) {
    return (
      <PersonaPanelShell>
        <div className="flex flex-1 items-center justify-center px-6 text-center text-sm text-[var(--text-muted)]">
          The agent will draft the persona here after its first pass
        </div>
      </PersonaPanelShell>
    );
  }

  if (personaQuery.isPending && !approvedPersona) {
    return <PersonaArtifactSkeleton />;
  }

  if (personaQuery.error || !persona) {
    return (
      <PersonaPanelShell>
        <div role="alert" className="m-5 rounded-md border border-[var(--status-error-border)] p-3 text-sm text-[var(--status-error)]">
          {personaQuery.error?.message ?? "Persona unavailable"}
        </div>
      </PersonaPanelShell>
    );
  }

  const approveDraft = async (asNew: boolean) => {
    const approved = asNew
      ? await approveAsNew.mutateAsync({ id: persona.id })
      : await approve.mutateAsync(persona.id);
    setSelectedArtifactVersion(null);
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

  return (
    <PersonaPanelShell>
      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="truncate text-base font-semibold text-[var(--text-primary)]">
                {persona.name}
              </h3>
              <span className="rounded-full bg-[var(--bg-elevated)] px-2 py-0.5 text-[11px] text-[var(--text-secondary)]">
                {persona.projectId ? "Project" : "Global"}
              </span>
              <StatusPill status={persona.status} />
            </div>
            <p className="mt-1 font-mono text-xs text-[var(--text-muted)]">{persona.slug}</p>
          </div>
        </div>

        {persona.artifactId ? (
          <div className="mt-4">
            <PersonaVersionHistory
              artifactId={persona.artifactId}
              currentContent={persona.content}
              selectedVersion={selectedArtifactVersion}
              onSelectedVersionChange={setSelectedArtifactVersion}
            />
          </div>
        ) : (
          <article className="prose prose-sm mt-5 max-w-none text-[var(--text-primary)] prose-headings:text-[var(--text-primary)] prose-p:text-[var(--text-secondary)]">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{persona.content}</ReactMarkdown>
          </article>
        )}

        {mutationError && (
          <div role="alert" className="mt-4 text-sm text-[var(--status-error)]">
            {mutationError.message}
          </div>
        )}

        {!isHistorical && (
          <div className="mt-6 flex flex-wrap gap-2 border-t border-[var(--border-subtle)] pt-4">
            {isDraft ? (
              <>
                <Button type="button" size="sm" disabled={isMutating} onClick={() => void approveDraft(false)}>
                  Approve persona
                </Button>
                {seededDraft && (
                  <Button type="button" size="sm" variant="outline" disabled={isMutating} onClick={() => void approveDraft(true)}>
                    Approve as new
                  </Button>
                )}
              </>
            ) : (
              <>
                <Button type="button" size="sm" variant="outline" onClick={() => openModal("settings", { section: "personas" })}>
                  Open in Settings
                </Button>
                {globalRefineDisabled ? (
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
                )}
              </>
            )}
          </div>
        )}
      </div>
    </PersonaPanelShell>
  );
}
