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
import { useAgentGate } from "@/hooks/useAgentGate";

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
  const recoveryGate = useAgentGate("recoveryPromptResolve");
  const showHostOnlyNotice = isRemoteEnvironment && recoveryGate.status === "unavailable";

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
        const applied = await resolveRecoveryPrompt(prompt.taskId, action);
        if (!applied && isRemoteEnvironment) {
          toast.info("Recovery was resolved on the host");
        }
        clearPrompt();
      } catch {
        toast.error("Failed to apply recovery action");
      } finally {
        setIsSubmitting(false);
      }
    },
    [prompt, clearPrompt, isRemoteEnvironment]
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
        {showHostOnlyNotice ? (
          <RemoteHostOnlyNotice subject="Recovery actions" />
        ) : null}
        <DialogFooter className="gap-2 sm:gap-2">
          {showHostOnlyNotice ? (
            <Button type="button" onClick={clearPrompt}>Dismiss</Button>
          ) : (
            <>
          <Button
            type="button"
            variant="secondary"
            onClick={() => handleAction(prompt.secondaryAction.id)}
            disabled={isSubmitting || recoveryGate.status !== "enabled"}
          >
            {prompt.secondaryAction.label}
          </Button>
          <Button
            type="button"
            onClick={() => handleAction(prompt.primaryAction.id)}
            disabled={isSubmitting || recoveryGate.status !== "enabled"}
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
