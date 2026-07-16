import { useMemo, useState } from "react";
import { ArrowLeft, Info } from "lucide-react";

import { PersonaBuilderView } from "@/components/personas/PersonaBuilderView";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  useApprovePersona,
  useArchivePersona,
  useCreatePersonaDraft,
  useDeletePersonaDraft,
  usePersonas,
  useUpdatePersona,
} from "@/hooks/usePersonas";
import { useConfirmation } from "@/hooks/useConfirmation";
import {
  isPersonaFeatureDisabledError,
  isPersonaUnavailableError,
} from "@/lib/personaErrors";
import { useProjectStore } from "@/stores/projectStore";
import type { Persona } from "@/types/persona";
import { splitPersonaBody } from "@/lib/personaContent";

import { PersonaRow, ScopeBadge } from "./PersonaManagementRows";

type EditorState =
  | { kind: "create" }
  | { kind: "edit"; persona: Persona };

type ProjectOption = { id: string; name: string };

export interface PersonasManagementSectionProps {
  /** Controls whether the builder entry is available in the management UI. */
  showBuilderEntry?: boolean;
}

function errorMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  if (isPersonaUnavailableError(message)) {
    return "This persona is no longer available.";
  }
  if (isPersonaFeatureDisabledError(message)) {
    return "Personas are disabled.";
  }
  return message || "Unable to save persona.";
}


function kebabCasePersonaName(name: string): string {
  return name
    .toLowerCase()
    .replace(/[\s_]+/g, "-")
    .replace(/[^a-z0-9-]/g, "")
    .replace(/-+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function PersonaEditor({
  editor,
  projects,
  projectNames,
  onBack,
}: {
  editor: EditorState;
  projects: ProjectOption[];
  projectNames: Record<string, string>;
  onBack: () => void;
}) {
  const editingDraft = editor.kind === "edit" && editor.persona.status === "draft";
  const [name, setName] = useState("");
  const [slug, setSlug] = useState(editor.kind === "create" ? "" : editor.persona.slug);
  const [slugTouched, setSlugTouched] = useState(false);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [description, setDescription] = useState(
    editor.kind === "create" ? "" : editor.persona.description,
  );
  const [instructions, setInstructions] = useState(
    editor.kind === "create" ? "" : splitPersonaBody(editor.persona.content),
  );
  const [saveError, setSaveError] = useState<string | null>(null);
  const createDraft = useCreatePersonaDraft();
  const updatePersona = useUpdatePersona();
  const isSaving = createDraft.isPending || updatePersona.isPending;
  const pastedPersonaDocument = instructions.startsWith("---");
  const requiredFieldsMissing =
    !slug.trim() ||
    !instructions.trim() ||
    (!pastedPersonaDocument && !description.trim());

  const handleSave = async () => {
    setSaveError(null);
    try {
      if (editor.kind === "create") {
        await createDraft.mutateAsync(
          pastedPersonaDocument
            ? { slug, projectId, content: instructions }
            : { slug, projectId, description, body: instructions },
        );
      } else {
        await updatePersona.mutateAsync(
          pastedPersonaDocument
            ? { id: editor.persona.id, content: instructions }
            : { id: editor.persona.id, description, body: instructions },
        );
      }
      onBack();
    } catch (error) {
      setSaveError(errorMessage(error));
    }
  };

  return (
    <section aria-label="Persona editor" className="space-y-5">
      <div className="flex items-center gap-2">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label="Back to personas"
              onClick={onBack}
              className="text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            >
              <ArrowLeft aria-hidden="true" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Back to personas</TooltipContent>
        </Tooltip>
        <div>
          <h3 className="text-sm font-semibold tracking-[-0.01em] text-[var(--text-primary)]">
            {editor.kind === "create" ? "New persona" : `Edit persona: ${editor.persona.name}`}
          </h3>
          {editor.kind === "edit" && (
            <p className="mt-1 text-xs text-[var(--text-muted)]">
              Name/slug: {editor.persona.slug} <span>(immutable once created)</span>
            </p>
          )}
        </div>
      </div>

      {editor.kind === "create" && (
        <>
          <div className="space-y-1.5">
            <Label htmlFor="persona-scope" className="text-xs text-[var(--text-secondary)]">
              Scope
            </Label>
            <select
              id="persona-scope"
              value={projectId ?? "global"}
              onChange={(event) =>
                setProjectId(event.target.value === "global" ? null : event.target.value)
              }
              disabled={isSaving}
              className="settings-input h-9 max-w-md rounded-md border border-[var(--border-default)] bg-[var(--bg-surface)] px-3 text-sm text-[var(--text-primary)]"
            >
              <option value="global">Global</option>
              {projects.map((project) => (
                <option key={project.id} value={project.id}>
                  {project.name}
                </option>
              ))}
            </select>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="persona-name" className="text-xs text-[var(--text-secondary)]">
              Name
            </Label>
            <Input
              id="persona-name"
              value={name}
              onChange={(event) => {
                const nextName = event.target.value;
                setName(nextName);
                if (!slugTouched) setSlug(kebabCasePersonaName(nextName));
              }}
              placeholder="Design voice"
              disabled={isSaving}
              className="settings-input max-w-md"
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="persona-slug" className="text-xs text-[var(--text-secondary)]">
              Slug
            </Label>
            <Input
              id="persona-slug"
              value={slug}
              onChange={(event) => {
                setSlugTouched(true);
                setSlug(event.target.value);
              }}
              placeholder="design-voice"
              disabled={isSaving}
              className="settings-input max-w-md"
            />
            <p className="text-xs text-[var(--text-muted)]">
              Generated from Name until you edit it directly.
            </p>
          </div>
        </>
      )}

      {editor.kind === "edit" && (
        <div className="space-y-1.5">
          <Label className="text-xs text-[var(--text-secondary)]">Scope</Label>
          <div data-testid="persona-editor-scope">
            <ScopeBadge projectId={editor.persona.projectId} projectNames={projectNames} />
          </div>
        </div>
      )}

      {editingDraft && (
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2 text-xs text-[var(--text-secondary)]">
          Drafts are iterated with the builder agent
        </div>
      )}

      <div className="space-y-1.5">
        <Label htmlFor="persona-description" className="text-xs text-[var(--text-secondary)]">
          Description
        </Label>
        <Input
          id="persona-description"
          value={description}
          onChange={(event) => setDescription(event.target.value)}
          placeholder="Opinionated product-design voice"
          disabled={editingDraft || isSaving}
          className="settings-input max-w-md"
        />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="persona-instructions" className="text-xs text-[var(--text-secondary)]">
          Instructions
        </Label>
        <textarea
          id="persona-instructions"
          value={instructions}
          onChange={(event) => setInstructions(event.target.value)}
          disabled={editingDraft || isSaving}
          placeholder="Plain Markdown. How should the agent behave, what tone should it use, and what should it avoid? No YAML needed."
          className="min-h-64 w-full resize-y rounded-md border border-[var(--border-default)] bg-[var(--bg-surface)] px-3 py-2 font-mono text-xs leading-relaxed text-[var(--text-primary)] outline-none placeholder:text-[var(--text-subtle)] focus:border-[var(--accent-primary)] focus:ring-1 focus:ring-[var(--accent-primary)] disabled:cursor-not-allowed disabled:opacity-70"
        />
      </div>

      {saveError && (
        <div
          role="alert"
          className="rounded-md border border-[var(--status-error-border)] bg-[var(--status-error-muted)] px-3 py-2 text-sm text-[var(--status-error)]"
        >
          Save failed: {saveError}
        </div>
      )}

      <div className="flex justify-end gap-2">
        <Button type="button" variant="ghost" onClick={onBack} disabled={isSaving}>
          Cancel
        </Button>
        {!editingDraft && (
          <Button
            type="button"
            onClick={() => void handleSave()}
            disabled={isSaving || requiredFieldsMissing}
            className="bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-secondary)]"
          >
            Save
          </Button>
        )}
      </div>
    </section>
  );
}


export function PersonasManagementSection({
  showBuilderEntry = true,
}: PersonasManagementSectionProps) {
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [scopeFilter, setScopeFilter] = useState("all");
  const [showBuilder, setShowBuilder] = useState(false);
  const [builderEntryError, setBuilderEntryError] = useState<string | null>(null);
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
  const { data: personas = [], error, isLoading } = usePersonas({ type: "all" });
  const approvePersona = useApprovePersona();
  const archivePersona = useArchivePersona();
  const deleteDraft = useDeletePersonaDraft();
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const visiblePersonas = personas.filter(
    (persona) =>
      persona.status !== "archived" &&
      (scopeFilter === "all" ||
        (scopeFilter === "global"
          ? persona.projectId === null
          : persona.projectId === scopeFilter)),
  );

  if (showBuilder && activeProjectId) {
    return (
      <PersonaBuilderView
        projectId={activeProjectId}
        onBack={() => setShowBuilder(false)}
      />
    );
  }

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
          {showBuilderEntry && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => {
                if (!activeProjectId) {
                  setBuilderEntryError("Select a project before building a persona.");
                  return;
                }
                setBuilderEntryError(null);
                setShowBuilder(true);
              }}
            >
              Build with agent
            </Button>
          )}
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
          {errorMessage(error)}
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
            />
          ))}
        </ul>
      )}

      {builderEntryError && (
        <div
          role="alert"
          className="rounded-md border border-[var(--status-error-border)] bg-[var(--status-error-muted)] px-3 py-2 text-sm text-[var(--status-error)]"
        >
          {builderEntryError}
        </div>
      )}

      <div className="flex gap-2 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-3 text-xs leading-relaxed text-[var(--text-muted)]">
        <Info className="mt-0.5 h-4 w-4 shrink-0 text-[var(--text-secondary)]" aria-hidden="true" />
        <p>
          v1 limits: applies to this conversation only — not to delegated, subagent, or pipeline work. Ideation, task, and merge chats and external API sends run without personas.
        </p>
      </div>
      <ConfirmationDialog {...confirmationDialogProps} />
    </section>
  );
}
