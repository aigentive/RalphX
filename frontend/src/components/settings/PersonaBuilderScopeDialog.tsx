import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface PersonaBuilderScopeDialogProps {
  open: boolean;
  projects: Array<{ id: string; name: string }>;
  initialProjectId: string | null;
  standaloneConversations: boolean;
  onClose: () => void;
  onStart: (projectId: string | null) => void;
}

export function PersonaBuilderScopeDialog({
  open,
  projects,
  initialProjectId,
  standaloneConversations,
  onClose,
  onStart,
}: PersonaBuilderScopeDialogProps) {
  const [scope, setScope] = useState<"global" | "project">(
    standaloneConversations ? "global" : "project",
  );
  const [projectId, setProjectId] = useState<string | null>(initialProjectId);

  useEffect(() => {
    if (!open) return;
    setScope(standaloneConversations ? "global" : "project");
    setProjectId(initialProjectId);
  }, [initialProjectId, open, standaloneConversations]);

  const selectedProjectId = scope === "project" ? projectId : null;

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && onClose()}>
      <DialogContent>
        <DialogHeader className="block space-y-1">
          <DialogTitle>Build persona with agent</DialogTitle>
          <DialogDescription>Where should this persona be available?</DialogDescription>
        </DialogHeader>
        <div className="space-y-4 px-6 py-5 text-sm">
          {standaloneConversations && (
            <label className="flex cursor-pointer items-start gap-3">
              <input
                type="radio"
                name="persona-builder-scope"
                checked={scope === "global"}
                onChange={() => setScope("global")}
                className="mt-1 accent-[var(--accent-primary)]"
              />
              <span>
                <span className="font-medium text-[var(--text-primary)]">Global — all projects</span>
                <span className="mt-1 block text-xs leading-relaxed text-[var(--text-muted)]">
                  Runs in a private workspace. Attach files or folders in the chat to give the agent context.
                </span>
              </span>
            </label>
          )}
          <label className="flex cursor-pointer items-start gap-3">
            <input
              type="radio"
              name="persona-builder-scope"
              checked={scope === "project"}
              onChange={() => setScope("project")}
              className="mt-2 accent-[var(--accent-primary)]"
            />
            <span className="min-w-0 flex-1">
              <span className="flex items-center gap-2 font-medium text-[var(--text-primary)]">
                Project:
                <select
                  aria-label="Build persona project"
                  value={projectId ?? ""}
                  onChange={(event) => {
                    setProjectId(event.target.value || null);
                    setScope("project");
                  }}
                  className="settings-input h-8 min-w-0 flex-1 rounded-md border border-[var(--border-default)] bg-[var(--bg-surface)] px-2 text-xs text-[var(--text-primary)]"
                >
                  <option value="">Select project…</option>
                  {projects.map((project) => (
                    <option key={project.id} value={project.id}>{project.name}</option>
                  ))}
                </select>
              </span>
              <span className="mt-1 block text-xs leading-relaxed text-[var(--text-muted)]">
                The agent may also analyze this project's code and docs for persona-relevant signals.
              </span>
            </span>
          </label>
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" onClick={onClose}>Cancel</Button>
          <Button
            type="button"
            disabled={scope === "project" && selectedProjectId === null}
            onClick={() => onStart(selectedProjectId)}
          >
            Start build
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
