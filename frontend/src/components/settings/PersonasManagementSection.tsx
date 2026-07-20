import { useEffect, useMemo, useRef, useState } from "react";
import { Info } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  useApprovePersona,
  useArchivePersona,
  useDeletePersonaDraft,
  usePersonas,
} from "@/hooks/usePersonas";
import { useConfirmation } from "@/hooks/useConfirmation";
import { formatPersonaErrorMessage } from "@/lib/personaErrors";
import { useProjectStore } from "@/stores/projectStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";
import type { Persona } from "@/types/persona";

import { PersonaEditor, type PersonaEditorState } from "./PersonaEditor";
import { PersonaBuilderScopeDialog } from "./PersonaBuilderScopeDialog";
import { PersonaRow } from "./PersonaManagementRows";

export interface PersonasManagementSectionProps {
  standaloneConversations?: boolean;
}

export function PersonasManagementSection({
  standaloneConversations = true,
}: PersonasManagementSectionProps) {
  const [editor, setEditor] = useState<PersonaEditorState | null>(null);
  const [scopeFilter, setScopeFilter] = useState("all");
  const [showScopeChooser, setShowScopeChooser] = useState(false);
  const handledDeepLinkKeyRef = useRef<string | null>(null);
  const activeProjectId = useProjectStore((state) => state.activeProjectId);
  const projectsById = useProjectStore((state) => state.projects);
  const projects = useMemo(
    () =>
      Object.values(projectsById)
        .map(({ id, name }) => ({ id, name }))
        .sort((left, right) => left.name.localeCompare(right.name)),
    [projectsById],
  );
  const projectNames = useMemo(
    () => Object.fromEntries(projects.map((project) => [project.id, project.name])),
    [projects],
  );
  const {
    data: personas = [],
    error,
    isFetching,
    isLoading,
  } = usePersonas({ type: "all" });
  const approvePersona = useApprovePersona();
  const archivePersona = useArchivePersona();
  const deleteDraft = useDeletePersonaDraft();
  const setStartConversationDraft = useAgentSessionStore((state) => state.setStartConversationDraft);
  const setFocusedProject = useAgentSessionStore((state) => state.setFocusedProject);
  const clearSelection = useAgentSessionStore((state) => state.clearSelection);
  const closeModal = useUiStore((state) => state.closeModal);
  const modalContext = useUiStore((state) => state.modalContext);
  const setCurrentView = useUiStore((state) => state.setCurrentView);
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const visiblePersonas = personas.filter(
    (persona) =>
      persona.status !== "archived" &&
      (scopeFilter === "all" ||
        (scopeFilter === "global"
          ? persona.projectId === null
          : persona.projectId === scopeFilter)),
  );

  useEffect(() => {
    const requestedPersonaId =
      modalContext?.["section"] === "personas" &&
      typeof modalContext["personaId"] === "string"
        ? modalContext["personaId"]
        : null;
    const requestedConversationId =
      typeof modalContext?.["conversationId"] === "string"
        ? modalContext["conversationId"]
        : null;
    if (!requestedPersonaId) {
      handledDeepLinkKeyRef.current = null;
      return;
    }
    const requestedDeepLinkKey = `${requestedPersonaId}:${requestedConversationId ?? ""}`;
    if (
      error ||
      isFetching ||
      handledDeepLinkKeyRef.current === requestedDeepLinkKey
    ) {
      return;
    }

    handledDeepLinkKeyRef.current = requestedDeepLinkKey;
    const requestedPersona = personas.find(
      (persona) => persona.id === requestedPersonaId,
    );
    if (
      requestedPersona?.status === "draft" ||
      requestedPersona?.status === "active"
    ) {
      setEditor({
        kind: "edit",
        persona: requestedPersona,
        ...(requestedConversationId && {
          conversationId: requestedConversationId,
        }),
      });
    }
  }, [error, isFetching, modalContext, personas]);

  const openBuilderComposer = (
    projectId: string | null,
    sourcePersona?: Persona,
  ) => {
    setStartConversationDraft({
      projectId,
      projectLocked: true,
      mode: "persona_builder",
      ...(sourcePersona
        ? {
            sourcePersonaId: sourcePersona.id,
            sourcePersonaName: sourcePersona.name,
          }
        : {}),
    });
    closeModal();
    setFocusedProject(projectId ?? null);
    clearSelection();
    setCurrentView("agents");
  };

  const handleEdit = (persona: Persona) => {
    if (persona.status === "draft") {
      setEditor({ kind: "edit", persona });
      return;
    }
    if (persona.status === "active") {
      setEditor({ kind: "edit", persona });
      return;
    }
  };

  const handleRemove = async (persona: Persona) => {
    const active = persona.status === "active";
    await confirm({
      title: active ? `Archive ${persona.name}?` : `Delete ${persona.name}?`,
      description: active
        ? "Archive clears conversation bindings for this persona."
        : "This permanently deletes the draft persona.",
      confirmText: active ? "Archive persona" : "Delete draft",
      pendingText: active ? "Archiving..." : "Deleting...",
      variant: "destructive",
      onConfirm: async () => {
        if (active) {
          await archivePersona.mutateAsync(persona.id);
        } else {
          await deleteDraft.mutateAsync(persona.id);
        }
      },
    });
  };

  if (editor) {
    return (
      <PersonaEditor
        editor={editor}
        projects={projects}
        projectNames={projectNames}
        onBack={() => setEditor(null)}
      />
    );
  }

  const emptyCaption =
    scopeFilter === "all"
      ? "No personas yet. Create a draft to get started."
      : scopeFilter === "global"
        ? "No global personas."
        : `No personas for ${projectNames[scopeFilter] ?? scopeFilter}.`;

  return (
    <section aria-labelledby="personas-heading" className="space-y-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 id="personas-heading" className="text-sm font-semibold tracking-[-0.01em] text-[var(--text-primary)]">
            Personas
          </h3>
          <p className="mt-1 text-sm text-[var(--text-secondary)]">
            Craft reusable voices for project agents.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => {
              setShowScopeChooser(true);
            }}
          >
            Build with Agent
          </Button>
          <Button
            type="button"
            size="sm"
            aria-label="New persona"
            onClick={() => setEditor({ kind: "create" })}
            className="bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-secondary)]"
          >
            + New
          </Button>
        </div>
      </div>

      <div className="flex items-center gap-2">
        <Label htmlFor="persona-scope-filter" className="text-xs text-[var(--text-secondary)]">
          Scope:
        </Label>
        <select
          id="persona-scope-filter"
          aria-label="Scope filter"
          value={scopeFilter}
          onChange={(event) => setScopeFilter(event.target.value)}
          className="settings-input h-8 rounded-md border border-[var(--border-default)] bg-[var(--bg-surface)] px-2 text-xs text-[var(--text-primary)]"
        >
          <option value="all">All</option>
          <option value="global">Global</option>
          {projects.map((project) => (
            <option key={project.id} value={project.id}>
              {project.name}
            </option>
          ))}
        </select>
      </div>

      {error ? (
        <div role="alert" className="rounded-md border border-[var(--status-error-border)] bg-[var(--status-error-muted)] px-3 py-2 text-sm text-[var(--status-error)]">
          {formatPersonaErrorMessage(error)}
        </div>
      ) : isLoading ? (
        <div className="h-28 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]" aria-label="Loading personas" />
      ) : visiblePersonas.length === 0 ? (
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-4 py-6 text-sm text-[var(--text-muted)]">
          {emptyCaption}
        </div>
      ) : (
        <ul className="divide-y divide-[var(--border-subtle)] rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]">
          {visiblePersonas.map((persona) => (
            <PersonaRow
              key={persona.id}
              persona={persona}
              projectNames={projectNames}
              onEdit={handleEdit}
              onActivate={(selected) => void approvePersona.mutateAsync(selected.id)}
              onRemove={(selected) => void handleRemove(selected)}
              onRefine={(selected) => openBuilderComposer(selected.projectId, selected)}
              {...(persona.projectId === null && !standaloneConversations
                ? {
                    refineDisabledReason:
                      "Global persona refinement requires standalone conversations",
                  }
                : {})}
            />
          ))}
        </ul>
      )}

      <div className="flex gap-2 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-3 text-xs leading-relaxed text-[var(--text-muted)]">
        <Info className="mt-0.5 h-4 w-4 shrink-0 text-[var(--text-secondary)]" aria-hidden="true" />
        <p>
          v1 limits: applies to this conversation only — not to delegated, subagent, or pipeline work. Ideation, task, and merge chats and external API sends run without personas.
        </p>
      </div>
      <ConfirmationDialog {...confirmationDialogProps} />
      <PersonaBuilderScopeDialog
        open={showScopeChooser}
        projects={projects}
        initialProjectId={activeProjectId ?? (!standaloneConversations ? projects[0]?.id ?? null : null)}
        standaloneConversations={standaloneConversations}
        onClose={() => setShowScopeChooser(false)}
        onStart={openBuilderComposer}
      />
    </section>
  );
}
