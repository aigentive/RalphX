/**
 * RecoveryPromptDialog - Handles backend recovery prompts
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { resolveRecoveryPrompt, type RecoveryAction } from "@/api/recovery";
import { useUiStore } from "@/stores/uiStore";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { RemoteHostOnlyNotice } from "@/components/remote/RemoteHostOnlyNotice";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";

interface RecoveryPromptDialogProps {
  taskId?: string | undefined;
  surface: "chat" | "task_detail";
}

export function RecoveryPromptDialog({
  taskId,
  surface,
}: RecoveryPromptDialogProps) {
  const prompt = useUiStore((s) => s.recoveryPrompt);
  const promptSurface = useUiStore((s) => s.recoveryPromptSurface);
  const setPromptSurface = useUiStore((s) => s.setRecoveryPromptSurface);
  const clearPrompt = useUiStore((s) => s.clearRecoveryPrompt);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const isRemoteEnvironment = useIsRemoteEnvironment();

  useEffect(() => {
    if (!prompt || !taskId || prompt.taskId !== taskId) return;
    if (!promptSurface) {
      setPromptSurface(surface);
    }
  }, [prompt, taskId, promptSurface, setPromptSurface, surface]);

  const isOpen = useMemo(() => {
    if (!prompt || !taskId) return false;
    return prompt.taskId === taskId && promptSurface === surface;
  }, [prompt, taskId, promptSurface, surface]);

  const handleAction = useCallback(
    async (action: RecoveryAction) => {
      if (!prompt) return;
      setIsSubmitting(true);
      try {
        await resolveRecoveryPrompt(prompt.taskId, action);
        clearPrompt();
      } catch {
        toast.error("Failed to apply recovery action");
      } finally {
        setIsSubmitting(false);
      }
    },
    [prompt, clearPrompt]
  );

  if (!prompt) {
    return null;
  }

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open) {
          clearPrompt();
        }
      }}
    >
      <DialogContent className="sm:max-w-[420px]">
        <DialogHeader>
          <DialogTitle>Recovery required</DialogTitle>
          <DialogDescription>{prompt.reason}</DialogDescription>
        </DialogHeader>
        {isRemoteEnvironment ? (
          <RemoteHostOnlyNotice subject="Recovery actions" />
        ) : null}
        <DialogFooter className="gap-2 sm:gap-2">
          {isRemoteEnvironment ? (
            <Button type="button" onClick={clearPrompt}>Dismiss</Button>
          ) : (
            <>
          <Button
            type="button"
            variant="secondary"
            onClick={() => handleAction(prompt.secondaryAction.id)}
            disabled={isSubmitting}
          >
            {prompt.secondaryAction.label}
          </Button>
          <Button
            type="button"
            onClick={() => handleAction(prompt.primaryAction.id)}
            disabled={isSubmitting}
          >
            {prompt.primaryAction.label}
          </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
