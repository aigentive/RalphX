import { useQuery } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { artifactApi } from "@/api/artifact";
import { PlanDisplay } from "@/components/Ideation/PlanDisplay";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { ArtifactSummary } from "@/types/task-context";

interface TaskDetailPlanDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  planArtifact: ArtifactSummary | null;
}

export function TaskDetailPlanDialog({
  open,
  onOpenChange,
  planArtifact,
}: TaskDetailPlanDialogProps) {
  const { data: artifact, isLoading } = useQuery({
    queryKey: ["task-detail-context", "plan-artifact", planArtifact?.id] as const,
    queryFn: async () => artifactApi.get(planArtifact!.id),
    enabled: open && Boolean(planArtifact?.id),
    staleTime: 30_000,
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex w-[min(1120px,calc(100vw-48px))] max-h-[calc(100vh-48px)] max-w-none flex-col overflow-hidden p-0">
        <DialogHeader>
          <div>
            <DialogTitle>Implementation Plan</DialogTitle>
            <DialogDescription>
              Latest plan linked to this task.
            </DialogDescription>
          </div>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
          {isLoading ? (
            <div className="flex items-center justify-center py-16">
              <Loader2 className="w-6 h-6 animate-spin text-text-primary/35" />
            </div>
          ) : artifact ? (
            <PlanDisplay
              plan={artifact}
              isExpanded={true}
              onExpandedChange={() => {}}
              showOverflowActions={false}
            />
          ) : (
            <div className="rounded-xl border border-[var(--overlay-weak)] bg-[var(--overlay-faint)] px-4 py-5 text-[0.8125rem] text-text-primary/55">
              The full plan could not be loaded.
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
