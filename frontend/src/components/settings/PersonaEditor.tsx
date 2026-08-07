import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ArrowLeft } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { PersonaContentDiff } from "@/components/personas/PersonaContentDiff";
import { PersonaVersionHistory } from "@/components/personas/PersonaVersionHistory";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useConversationSummary } from "@/hooks/useChat";
import { useAgentGate } from "@/hooks/useAgentGate";
import {
  fetchPersona,
  personaKeys,
  useCreatePersonaDraft,
  useUpdatePersona,
  useUpdatePersonaDraft,
} from "@/hooks/usePersonas";
import { navigateToAgentConversation } from "@/lib/navigation";
import { composePersonaContent, splitPersonaBody } from "@/lib/personaContent";
import { formatPersonaErrorMessage } from "@/lib/personaErrors";
import { useUiStore } from "@/stores/uiStore";
import type { Persona } from "@/types/persona";

import { ScopeBadge } from "./PersonaManagementRows";

export type PersonaEditorState =
  | { kind: "create" }
  | { kind: "edit"; persona: Persona; conversationId?: string };

type ProjectOption = { id: string; name: string };

const PERSONA_DRAFT_CONFLICT_PREFIX = "PERSONA_DRAFT_CONFLICT:";

function kebabCasePersonaName(name: string): string {
  return name
    .toLowerCase()
    .replace(/[\s_]+/g, "-")
    .replace(/[^a-z0-9-]/g, "")
    .replace(/-+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export function PersonaEditor({
  editor,
  projects,
  projectNames,
  onBack,
}: {
  editor: PersonaEditorState;
  projects: ProjectOption[];
  projectNames: Record<string, string>;
  onBack: () => void;
}) {
  const [currentPersona, setCurrentPersona] = useState(
    editor.kind === "edit" ? editor.persona : null,
  );
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
  const [instructionsTab, setInstructionsTab] = useState<
    "write" | "preview" | "diff"
  >("write");
  const [saveError, setSaveError] = useState<string | null>(null);
  const [draftConflict, setDraftConflict] = useState(false);
  const [isReloading, setIsReloading] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const queryClient = useQueryClient();
  const createDraft = useCreatePersonaDraft();
  const updatePersona = useUpdatePersona();
  const updateDraft = useUpdatePersonaDraft();
  const createGate = useAgentGate("personaCreateDraft");
  const updateGate = useAgentGate("personaUpdate");
  const updateDraftGate = useAgentGate("personaUpdateDraft");
  const closeModal = useUiStore((state) => state.closeModal);
  const builderConversationId =
    (editor.kind === "edit" ? editor.conversationId : undefined) ??
    currentPersona?.sourceSessionId ??
    null;
  const builderConversation = useConversationSummary(builderConversationId);
  const isSaving = createDraft.isPending || updatePersona.isPending || updateDraft.isPending;
  const pastedPersonaDocument = instructions.startsWith("---");
  const requiredFieldsMissing =
    !slug.trim() ||
    !instructions.trim() ||
    (!pastedPersonaDocument && !description.trim());
  const saveGate =
    editor.kind === "create"
      ? createGate
      : currentPersona?.status === "draft"
        ? updateDraftGate
        : updateGate;

  const handleSave = async () => {
    if (saveGate.gated) return;
    setSaveError(null);
    setDraftConflict(false);
    try {
      if (editor.kind === "create") {
        await createDraft.mutateAsync(
          pastedPersonaDocument
            ? { slug, projectId, content: instructions }
            : { slug, projectId, description, body: instructions },
        );
      } else if (currentPersona?.status === "draft") {
        await updateDraft.mutateAsync({
          id: currentPersona.id,
          content: pastedPersonaDocument
            ? instructions
            : composePersonaContent(currentPersona.slug, description, instructions),
          expectedContentHash: currentPersona.contentHash,
        });
      } else {
        await updatePersona.mutateAsync(
          pastedPersonaDocument
            ? { id: editor.persona.id, content: instructions }
            : { id: editor.persona.id, description, body: instructions },
        );
      }
      onBack();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (message.includes(PERSONA_DRAFT_CONFLICT_PREFIX)) {
        setDraftConflict(true);
      } else {
        setSaveError(formatPersonaErrorMessage(error));
      }
    }
  };

  const handleReloadDraft = async () => {
    if (!currentPersona) return;
    setIsReloading(true);
    setSaveError(null);
    try {
      await queryClient.invalidateQueries({
        queryKey: personaKeys.detail(currentPersona.id),
      });
      const freshPersona = await queryClient.fetchQuery({
        queryKey: personaKeys.detail(currentPersona.id),
        queryFn: () => fetchPersona(currentPersona.id),
      });
      setCurrentPersona(freshPersona);
      setDescription(freshPersona.description);
      setInstructions(splitPersonaBody(freshPersona.content));
      setDraftConflict(false);
    } catch (error) {
      setSaveError(formatPersonaErrorMessage(error));
    } finally {
      setIsReloading(false);
    }
  };

  const handleOpenBuilderConversation = () => {
    if (!builderConversationId || !builderConversation.data) return;
    if (builderConversation.data.contextType === "project") {
      closeModal();
      navigateToAgentConversation(
        builderConversation.data.contextId,
        builderConversationId,
      );
      return;
    }
    if (builderConversation.data.contextType === "standalone") {
      closeModal();
      navigateToAgentConversation(null, builderConversationId);
    }
  };
  const canOpenBuilderConversation = Boolean(
    builderConversation.data &&
      (builderConversation.data.contextType === "standalone" ||
        (builderConversation.data.contextType === "project" &&
          builderConversation.data.contextId)),
  );

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
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold tracking-[-0.01em] text-[var(--text-primary)]">
            {editor.kind === "create" ? "New persona" : `Edit persona: ${editor.persona.name}`}
          </h3>
          {editor.kind === "edit" && (
            <p className="mt-1 text-xs text-[var(--text-muted)]">
              Name/slug: {editor.persona.slug} <span>(immutable once created)</span>
            </p>
          )}
        </div>
        {currentPersona?.artifactId && (
          <Button
            type="button"
            variant="link"
            size="sm"
            onClick={() => setHistoryOpen(true)}
            className="h-auto shrink-0 p-0 text-xs text-[var(--text-secondary)]"
          >
            Version history
          </Button>
        )}
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

      {builderConversationId && (
        <div
          className="flex flex-wrap items-center gap-x-2 gap-y-1 rounded-md bg-[var(--bg-elevated)] px-3 py-2 text-xs text-[var(--text-muted)]"
          style={{
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: 1,
          }}
        >
          <span>Persona Builder conversation</span>
          <Button
            type="button"
            variant="link"
            size="sm"
            onClick={handleOpenBuilderConversation}
            disabled={!canOpenBuilderConversation}
            className="h-auto p-0 text-xs text-[var(--text-secondary)]"
          >
            Open in Agent
          </Button>
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
          disabled={isSaving}
          className="settings-input max-w-md"
        />
      </div>

      <div className="space-y-1.5">
        <div className="flex items-center justify-between gap-2">
          <Label htmlFor="persona-instructions" className="text-xs text-[var(--text-secondary)]">
            Instructions
          </Label>
          <div
            role="tablist"
            aria-label="Instructions view"
            className="inline-flex items-center gap-0.5 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-0.5"
          >
            {(
              [
                { value: "write", label: "Write" },
                { value: "preview", label: "Preview" },
                ...(currentPersona
                  ? [
                      {
                        value: "diff",
                        label: `Diff vs v${currentPersona.version}`,
                      } as const,
                    ]
                  : []),
              ] as { value: "write" | "preview" | "diff"; label: string }[]
            ).map((tab) => (
              <button
                key={tab.value}
                type="button"
                role="tab"
                aria-selected={instructionsTab === tab.value}
                onClick={() => setInstructionsTab(tab.value)}
                className={
                  instructionsTab === tab.value
                    ? "rounded px-2 py-1 text-xs font-medium bg-[var(--bg-surface)] text-[var(--text-primary)]"
                    : "rounded px-2 py-1 text-xs font-medium text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                }
              >
                {tab.label}
              </button>
            ))}
          </div>
        </div>
        {instructionsTab === "write" ? (
          <textarea
            id="persona-instructions"
            value={instructions}
            onChange={(event) => setInstructions(event.target.value)}
            disabled={isSaving}
            placeholder="Plain Markdown. How should the agent behave, what tone should it use, and what should it avoid? No YAML needed."
            className="min-h-[50vh] w-full resize-y rounded-md border border-[var(--border-default)] bg-[var(--bg-surface)] px-3 py-2 font-mono text-xs leading-relaxed text-[var(--text-primary)] outline-none placeholder:text-[var(--text-subtle)] focus:border-[var(--accent-primary)] focus:ring-1 focus:ring-[var(--accent-primary)] disabled:cursor-not-allowed disabled:opacity-70"
          />
        ) : instructionsTab === "preview" ? (
          <div
            role="tabpanel"
            aria-label="Instructions preview"
            data-testid="persona-instructions-preview"
            className="min-h-[50vh] w-full rounded-md border border-[var(--border-default)] bg-[var(--bg-surface)] px-3 py-2 text-sm leading-relaxed text-[var(--text-primary)]"
          >
            {instructions.trim() === "" ? (
              <p className="text-xs text-[var(--text-muted)]">Nothing to preview.</p>
            ) : (
              <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
                {instructions}
              </ReactMarkdown>
            )}
          </div>
        ) : (
          currentPersona && (
            <PersonaContentDiff
              oldContent={splitPersonaBody(currentPersona.content)}
              newContent={instructions}
              ariaLabel={`Changes against version ${currentPersona.version}`}
            />
          )
        )}
      </div>

      {saveError && (
        <div
          role="alert"
          className="rounded-md border border-[var(--status-error-border)] bg-[var(--status-error-muted)] px-3 py-2 text-sm text-[var(--status-error)]"
        >
          Save failed: {saveError}
        </div>
      )}

      {draftConflict && (
        <div
          role="alert"
          className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-[var(--status-warning-border)] bg-[var(--status-warning-muted)] px-3 py-2 text-sm text-[var(--text-primary)]"
        >
          <span>This draft changed since you loaded it.</span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void handleReloadDraft()}
            disabled={isReloading}
          >
            {isReloading ? "Reloading..." : "Reload draft"}
          </Button>
        </div>
      )}

      <div className="flex justify-end gap-2">
        <Button type="button" variant="ghost" onClick={onBack} disabled={isSaving}>
          Cancel
        </Button>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              onClick={() => void handleSave()}
              disabled={isSaving || requiredFieldsMissing}
              aria-disabled={saveGate.gated || undefined}
              data-disabled-explained={saveGate.gated ? "true" : undefined}
              className={`bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-secondary)]${saveGate.gated ? " opacity-50" : ""}`}
            >
              {currentPersona ? `Save (creates v${currentPersona.version + 1})` : "Save"}
            </Button>
          </TooltipTrigger>
          <TooltipContent>
            {saveGate.gated ? saveGate.reason : "Save persona"}
          </TooltipContent>
        </Tooltip>
      </div>

      {currentPersona?.artifactId && (
        <Dialog
          open={historyOpen}
          onOpenChange={setHistoryOpen}
        >
          <DialogContent className="max-w-4xl" aria-describedby="persona-history-description">
            <DialogHeader>
              <div>
                <DialogTitle>Version history</DialogTitle>
                <DialogDescription id="persona-history-description">
                  Attributed persona revisions. Historical versions are read-only.
                </DialogDescription>
              </div>
            </DialogHeader>
            <div className="max-h-[76vh] overflow-y-auto px-6 py-5">
              <PersonaVersionHistory artifactId={currentPersona.artifactId} />
            </div>
          </DialogContent>
        </Dialog>
      )}
    </section>
  );
}
