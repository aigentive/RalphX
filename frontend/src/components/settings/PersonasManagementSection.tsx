import { useEffect, useMemo, useRef, useState } from "react";
import { Info, Search } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  useApprovePersona,
  useArchivePersona,
  useDeletePersonaDraft,
  usePersonas,
  usePersonaUsage,
  useUnarchivePersona,
} from "@/hooks/usePersonas";
import { useConfirmation } from "@/hooks/useConfirmation";
import { formatPersonaErrorMessage } from "@/lib/personaErrors";
import { useProjectStore } from "@/stores/projectStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import type { Persona } from "@/types/persona";

import { PersonaEditor, type PersonaEditorState } from "./PersonaEditor";
import { PersonaBuilderScopeDialog } from "./PersonaBuilderScopeDialog";
import { PersonaRow, PersonaRowSkeleton } from "./PersonaManagementRows";

type PersonaStatusTab = "live" | "draft" | "archived";

const STATUS_TABS: { value: PersonaStatusTab; label: string }[] = [
  { value: "live", label: "All" },
  { value: "draft", label: "Drafts" },
  { value: "archived", label: "Archived" },
];

function personaMatchesTab(status: Persona["status"], tab: PersonaStatusTab): boolean {
  if (tab === "archived") return status === "archived";
  if (tab === "draft") return status === "draft";
  return status !== "archived";
}

export interface PersonasManagementSectionProps {
  standaloneConversations?: boolean;
}

export function PersonasManagementSection({
  standaloneConversations = true,
}: PersonasManagementSectionProps) {
  const [editor, setEditor] = useState<PersonaEditorState | null>(null);
  const [scopeFilter, setScopeFilter] = useState("all");
  const [statusTab, setStatusTab] = useState<PersonaStatusTab>("live");
  const [search, setSearch] = useState("");
  const [restoreError, setRestoreError] = useState<string | null>(null);
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
  const unarchivePersona = useUnarchivePersona();
  const deleteDraft = useDeletePersonaDraft();
  const usageQuery = usePersonaUsage();
  const usageByPersonaId = useMemo(
    () =>
      new Map((usageQuery.data ?? []).map((usage) => [usage.personaId, usage])),
    [usageQuery.data],
  );
  const setStartConversationDraft = useAgentSessionStore((state) => state.setStartConversationDraft);
  const setFocusedProject = useAgentSessionStore((state) => state.setFocusedProject);
  const clearSelection = useAgentSessionStore((state) => state.clearSelection);
  const setActiveConversation = useChatStore((state) => state.setActiveConversation);
  const closeModal = useUiStore((state) => state.closeModal);
  const modalContext = useUiStore((state) => state.modalContext);
  const setCurrentView = useUiStore((state) => state.setCurrentView);
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const normalizedSearch = search.trim().toLowerCase();
  const matchesScopeAndSearch = (persona: Persona) =>
    (scopeFilter === "all" ||
      (scopeFilter === "global"
        ? persona.projectId === null
        : persona.projectId === scopeFilter)) &&
    (normalizedSearch === "" ||
      persona.name.toLowerCase().includes(normalizedSearch) ||
      persona.slug.toLowerCase().includes(normalizedSearch) ||
      persona.description.toLowerCase().includes(normalizedSearch));
  const tabCounts: Record<PersonaStatusTab, number> = {
    live: 0,
    draft: 0,
    archived: 0,
  };
  for (const persona of personas) {
    if (!matchesScopeAndSearch(persona)) continue;
    for (const tab of STATUS_TABS) {
      if (personaMatchesTab(persona.status, tab.value)) tabCounts[tab.value] += 1;
    }
  }
  const visiblePersonas = personas.filter(
    (persona) =>
      personaMatchesTab(persona.status, statusTab) && matchesScopeAndSearch(persona),
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
    if (projectId) {
      setActiveConversation(`project:${projectId}`, null);
    }
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

  const handleRestore = async (persona: Persona) => {
    setRestoreError(null);
    try {
      await unarchivePersona.mutateAsync(persona.id);
    } catch (restoreFailure) {
      setRestoreError(formatPersonaErrorMessage(restoreFailure));
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
    normalizedSearch !== ""
      ? `No personas match "${search.trim()}".`
      : statusTab === "archived"
        ? "No archived personas."
        : statusTab === "draft"
          ? "No drafts."
          : scopeFilter === "all"
            ? "No personas yet. Create a draft to get started."
            : scopeFilter === "global"
              ? "No global personas."
              : `No personas for ${projectNames[scopeFilter] ?? scopeFilter}.`;
  const showEmptyCtas = normalizedSearch === "" && statusTab !== "archived";

  return (
    <section aria-labelledby="personas-heading" className="space-y-4">
      <h3 id="personas-heading" className="settings-section__label">
        Persona library
      </h3>

      <div className="flex flex-wrap items-center gap-2">
        <div className="relative min-w-44 flex-1">
          <Search
            className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[var(--text-muted)]"
            aria-hidden="true"
          />
          <Input
            type="search"
            aria-label="Search personas"
            placeholder="Search personas…"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            className="settings-input h-8 pl-8 text-xs"
          />
        </div>
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
        <div
          role="tablist"
          aria-label="Persona status"
          className="inline-flex items-center gap-0.5 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-0.5"
        >
          {STATUS_TABS.map((tab) => (
            <button
              key={tab.value}
              type="button"
              role="tab"
              aria-selected={statusTab === tab.value}
              onClick={() => setStatusTab(tab.value)}
              className={
                statusTab === tab.value
                  ? "rounded px-2 py-1 text-xs font-medium bg-[var(--bg-surface)] text-[var(--text-primary)]"
                  : "rounded px-2 py-1 text-xs font-medium text-[var(--text-muted)] hover:text-[var(--text-primary)]"
              }
            >
              {tab.label}
              {tabCounts[tab.value] > 0 && (
                <span className="ml-1 text-[10px] text-[var(--text-muted)]">
                  {tabCounts[tab.value]}
                </span>
              )}
            </button>
          ))}
        </div>
        <div className="ml-auto flex items-center gap-2">
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

      {restoreError && (
        <div
          role="alert"
          className="rounded-md border border-[var(--status-error-border)] bg-[var(--status-error-muted)] px-3 py-2 text-sm text-[var(--status-error)]"
        >
          {restoreError}
        </div>
      )}

      {error ? (
        <div role="alert" className="rounded-md border border-[var(--status-error-border)] bg-[var(--status-error-muted)] px-3 py-2 text-sm text-[var(--status-error)]">
          {formatPersonaErrorMessage(error)}
        </div>
      ) : isLoading ? (
        <ul
          aria-label="Loading personas"
          className="divide-y divide-[var(--border-subtle)] rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]"
        >
          <PersonaRowSkeleton />
          <PersonaRowSkeleton />
          <PersonaRowSkeleton />
        </ul>
      ) : visiblePersonas.length === 0 ? (
        <div className="space-y-3 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-4 py-6 text-sm text-[var(--text-muted)]">
          <p>{emptyCaption}</p>
          {showEmptyCtas && (
            <div className="flex gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => setShowScopeChooser(true)}
              >
                Build with Agent
              </Button>
              <Button
                type="button"
                size="sm"
                onClick={() => setEditor({ kind: "create" })}
                className="bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-secondary)]"
              >
                + New
              </Button>
            </div>
          )}
        </div>
      ) : (
        <ul className="divide-y divide-[var(--border-subtle)] rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]">
          {visiblePersonas.map((persona) => (
            <PersonaRow
              key={persona.id}
              persona={persona}
              projectNames={projectNames}
              usage={usageByPersonaId.get(persona.id)}
              usageError={usageQuery.isError}
              onEdit={handleEdit}
              onActivate={(selected) => void approvePersona.mutateAsync(selected.id)}
              onRemove={(selected) => void handleRemove(selected)}
              onRefine={(selected) => openBuilderComposer(selected.projectId, selected)}
              onRestore={(selected) => void handleRestore(selected)}
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
