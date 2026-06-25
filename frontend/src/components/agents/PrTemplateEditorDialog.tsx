import { useEffect, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";

import { projectsApi } from "@/api/projects";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import type { Project } from "@/types/project";

interface PrTemplateEditorDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  project: Project | null;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

export function PrTemplateEditorDialog({
  open,
  onOpenChange,
  project,
}: PrTemplateEditorDialogProps) {
  const [draftContent, setDraftContent] = useState("");
  const [saveError, setSaveError] = useState<string | null>(null);
  const projectId = project?.id;

  const templateQuery = useQuery({
    queryKey: ["pr-template", projectId],
    queryFn: () => projectsApi.readPrTemplate(projectId ?? ""),
    enabled: open && Boolean(projectId),
    staleTime: 0,
  });

  useEffect(() => {
    if (!open) {
      setSaveError(null);
      return;
    }
    if (templateQuery.isSuccess) {
      setDraftContent(templateQuery.data ?? "");
      setSaveError(null);
    }
  }, [open, templateQuery.data, templateQuery.isSuccess]);

  const saveMutation = useMutation({
    mutationFn: (content: string) => {
      if (!projectId) {
        throw new Error("Project is required");
      }
      return projectsApi.writePrTemplate(projectId, content);
    },
    onSuccess: () => {
      setSaveError(null);
      onOpenChange(false);
    },
    onError: (error) => {
      setSaveError(errorMessage(error));
    },
  });

  const isReading = templateQuery.isLoading || templateQuery.isFetching;
  const isSaving = saveMutation.isPending;
  const readError = templateQuery.error ? errorMessage(templateQuery.error) : null;
  const showMissingHint =
    templateQuery.isSuccess && templateQuery.data === null && !isReading;

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => {
      if (!isSaving) {
        onOpenChange(nextOpen);
      }
    }}>
      <DialogContent
        className="max-h-[92vh] max-w-3xl overflow-hidden"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <DialogHeader>
          <div>
            <DialogTitle>Edit PR Template</DialogTitle>
            <DialogDescription className="mt-1">
              Edit the GitHub pull request description template for {project?.name ?? "this project"}.
              RalphX reads existing `.github/pull_request_template.md` or legacy uppercase
              templates, and saves new templates to the lowercase path.
            </DialogDescription>
          </div>
        </DialogHeader>

        <div className="space-y-3 overflow-y-auto px-6 py-4">
          <div className="space-y-2">
            <Label htmlFor="pr-template-content">Pull request template</Label>
            <Textarea
              id="pr-template-content"
              aria-describedby="pr-template-status"
              className="min-h-[520px] resize-y font-mono text-sm"
              style={{
                backgroundColor: "var(--bg-base)",
                borderColor: "var(--border-subtle)",
              }}
              disabled={isReading || isSaving || !projectId}
              value={draftContent}
              onChange={(event) => setDraftContent(event.target.value)}
            />
          </div>

          <div
            id="pr-template-status"
            className="min-h-5 text-sm text-[var(--text-secondary)]"
            role={readError || saveError ? "alert" : undefined}
          >
            {isReading
              ? "Loading template..."
              : showMissingHint
                ? "Saving will create `.github/pull_request_template.md`."
                : null}
            {readError ? (
              <span className="text-[var(--status-error)]">{readError}</span>
            ) : null}
            {saveError ? (
              <span className="text-[var(--status-error)]">{saveError}</span>
            ) : null}
          </div>
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            disabled={isSaving}
            onClick={() => onOpenChange(false)}
          >
            Cancel
          </Button>
          <Button
            type="button"
            disabled={isReading || isSaving || !projectId}
            onClick={() => saveMutation.mutate(draftContent)}
          >
            {isSaving ? "Saving..." : "Save"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
