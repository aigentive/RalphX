import { useState } from "react";
import { ArrowLeft, Info, Trash2 } from "lucide-react";

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
import type { Persona } from "@/types/persona";

type EditorState =
  | { kind: "create" }
  | { kind: "edit"; persona: Persona };

export interface PersonasSectionProps {
  /** Reserved for the builder-agent slice; manual editing remains the default. */
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

function relativeUpdatedAt(updatedAt: string): string {
  const elapsedMs = Date.now() - new Date(updatedAt).getTime();
  const hours = Math.round(elapsedMs / (60 * 60 * 1000));
  if (hours < 1) return "Updated just now";
  if (hours < 24) return `Updated ${hours}h ago`;
  const days = Math.round(hours / 24);
  return days === 1 ? "Updated yesterday" : `Updated ${days}d ago`;
}

function StatusChip({ status }: { status: Persona["status"] }) {
  const active = status === "active";
  return (
    <span
      className={
        active
          ? "rounded-sm border border-[var(--accent-border)] bg-[var(--accent-muted)] px-1.5 py-0.5 text-[10px] font-semibold tracking-wide text-[var(--accent-primary)]"
          : "rounded-sm border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-1.5 py-0.5 text-[10px] font-semibold tracking-wide text-[var(--text-muted)]"
      }
    >
      {status.toUpperCase()}
    </span>
  );
}

function PersonaEditor({
  editor,
  onBack,
}: {
  editor: EditorState;
  onBack: () => void;
}) {
  const editingDraft = editor.kind === "edit" && editor.persona.status === "draft";
  const [slug, setSlug] = useState(editor.kind === "create" ? "" : editor.persona.slug);
  const [content, setContent] = useState(
    editor.kind === "create" ? "" : editor.persona.content,
  );
  const [saveError, setSaveError] = useState<string | null>(null);
  const createDraft = useCreatePersonaDraft();
  const updatePersona = useUpdatePersona();
  const isSaving = createDraft.isPending || updatePersona.isPending;

  const handleSave = async () => {
    setSaveError(null);
    try {
      if (editor.kind === "create") {
        await createDraft.mutateAsync({ slug, content });
      } else {
        await updatePersona.mutateAsync({ id: editor.persona.id, content });
      }
      onBack();
    } catch (error) {
      setSaveError(errorMessage(error));
    }
  };

  return (
    <section aria-label="Persona editor" className="space-y-5">
      <div className="flex items-center gap-2">
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
        <div className="space-y-1.5">
          <Label htmlFor="persona-slug" className="text-xs text-[var(--text-secondary)]">
            Slug
          </Label>
          <Input
            id="persona-slug"
            value={slug}
            onChange={(event) => setSlug(event.target.value)}
            placeholder="reviewer-voice"
            disabled={isSaving}
            className="settings-input max-w-md"
          />
        </div>
      )}

      {editingDraft && (
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2 text-xs text-[var(--text-secondary)]">
          Drafts are iterated with the builder agent
        </div>
      )}

      <div className="space-y-1.5">
        <Label htmlFor="persona-content" className="text-xs text-[var(--text-secondary)]">
          Persona content
        </Label>
        <textarea
          id="persona-content"
          value={content}
          onChange={(event) => setContent(event.target.value)}
          disabled={editingDraft || isSaving}
          placeholder="---\nname: reviewer-voice\n---\n\nPersona instructions in Markdown"
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
            disabled={isSaving || (editor.kind === "create" && (!slug.trim() || !content.trim()))}
            className="bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-secondary)]"
          >
            Save
          </Button>
        )}
      </div>
    </section>
  );
}

function PersonaRow({
  persona,
  onEdit,
  onActivate,
  onRemove,
}: {
  persona: Persona;
  onEdit: (persona: Persona) => void;
  onActivate: (persona: Persona) => void;
  onRemove: (persona: Persona) => void;
}) {
  const active = persona.status === "active";
  const actionLabel = active ? "Archive" : "Delete";
  return (
    <li className="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3">
      <span
        aria-label={active ? "Active persona" : "Draft persona"}
        className={
          active
            ? "h-2.5 w-2.5 rounded-full bg-[var(--accent-primary)]"
            : "h-2.5 w-2.5 rounded-full border border-[var(--text-muted)]"
        }
      />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
          <span className="font-medium text-[var(--text-primary)]">{persona.name}</span>
          <code className="text-xs text-[var(--text-muted)]">{persona.slug}</code>
          <StatusChip status={persona.status} />
          <span className="text-xs text-[var(--text-muted)]">v{persona.version}</span>
        </div>
        <p className="mt-1 text-xs text-[var(--text-muted)]">{relativeUpdatedAt(persona.updatedAt)}</p>
      </div>
      <div className="flex items-center gap-1">
        {!active && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-label={`Activate ${persona.name}`}
            onClick={() => onActivate(persona)}
            className="text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
          >
            Activate
          </Button>
        )}
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label={`Edit ${persona.name}`}
          onClick={() => onEdit(persona)}
          className="text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
        >
          Edit
        </Button>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={`${actionLabel} ${persona.name}`}
              onClick={() => onRemove(persona)}
              className="text-[var(--text-muted)] hover:bg-[var(--status-error-muted)] hover:text-[var(--status-error)]"
            >
              <Trash2 aria-hidden="true" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{actionLabel} {persona.name}</TooltipContent>
        </Tooltip>
      </div>
    </li>
  );
}

export function PersonasSection({ showBuilderEntry = false }: PersonasSectionProps) {
  const [editor, setEditor] = useState<EditorState | null>(null);
  const { data: personas = [], error, isLoading } = usePersonas();
  const approvePersona = useApprovePersona();
  const archivePersona = useArchivePersona();
  const deleteDraft = useDeletePersonaDraft();
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const visiblePersonas = personas.filter((persona) => persona.status !== "archived");

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
    return <PersonaEditor editor={editor} onBack={() => setEditor(null)} />;
  }

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
            <Button type="button" variant="outline" size="sm">
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

      {error ? (
        <div role="alert" className="rounded-md border border-[var(--status-error-border)] bg-[var(--status-error-muted)] px-3 py-2 text-sm text-[var(--status-error)]">
          {errorMessage(error)}
        </div>
      ) : isLoading ? (
        <div className="h-28 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]" aria-label="Loading personas" />
      ) : visiblePersonas.length === 0 ? (
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-4 py-6 text-sm text-[var(--text-muted)]">
          No personas yet. Create a draft to get started.
        </div>
      ) : (
        <ul className="divide-y divide-[var(--border-subtle)] rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]">
          {visiblePersonas.map((persona) => (
            <PersonaRow
              key={persona.id}
              persona={persona}
              onEdit={handleEdit}
              onActivate={(selected) => void approvePersona.mutateAsync(selected.id)}
              onRemove={(selected) => void handleRemove(selected)}
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
    </section>
  );
}
