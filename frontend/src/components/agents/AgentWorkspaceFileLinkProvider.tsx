import { useCallback, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { toast } from "sonner";

import { chatApi, type AgentConversationWorkspace } from "@/api/chat";
import { MessageFileLinkContext } from "@/components/Chat/MessageFileLinkContext";
import {
  readPreferredWorkspaceOpenTargetId,
  subscribePreferredWorkspaceOpenTargetId,
  writePreferredWorkspaceOpenTargetId,
} from "@/lib/workspace-open-targets";

function workspacePathOpenErrorMessage(error: unknown): string {
  if (typeof error === "string" && error.trim()) {
    return error;
  }
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }
  return "Failed to open file";
}

interface AgentWorkspaceFileLinkProviderProps {
  conversationId: string;
  workspace: AgentConversationWorkspace | null;
  children: ReactNode;
}

export function AgentWorkspaceFileLinkProvider({
  conversationId,
  workspace,
  children,
}: AgentWorkspaceFileLinkProviderProps) {
  const [preferredTargetId, setPreferredTargetId] = useState(
    readPreferredWorkspaceOpenTargetId,
  );
  const [opening, setOpening] = useState<{
    path: string;
    targetId: string;
  } | null>(null);
  const workspaceConversationId = workspace?.conversationId ?? conversationId;
  const workspaceOpenTargetsQuery = useQuery({
    queryKey: ["workspace-open-targets"],
    queryFn: chatApi.listWorkspaceOpenTargets,
    staleTime: Infinity,
    enabled: Boolean(workspace),
  });
  const targets = useMemo(
    () => workspaceOpenTargetsQuery.data ?? [],
    [workspaceOpenTargetsQuery.data],
  );
  const openWorkspacePathMutation = useMutation({
    mutationFn: ({ path, targetId }: { path: string; targetId: string }) =>
      chatApi.openAgentConversationWorkspacePath(
        workspaceConversationId,
        targetId,
        path,
      ),
    onMutate: ({ path, targetId }) => {
      setOpening({ path, targetId });
    },
    onSettled: () => {
      setOpening(null);
    },
    onError: (error) => {
      toast.error(workspacePathOpenErrorMessage(error));
    },
  });

  useEffect(
    () => subscribePreferredWorkspaceOpenTargetId(setPreferredTargetId),
    [],
  );

  useEffect(() => {
    if (
      targets.length > 0 &&
      preferredTargetId &&
      !targets.some((target) => target.id === preferredTargetId)
    ) {
      setPreferredTargetId(targets[0]?.id ?? null);
    }
  }, [preferredTargetId, targets]);

  const openPath = useCallback(
    (path: string, targetId: string) => {
      setPreferredTargetId(targetId);
      writePreferredWorkspaceOpenTargetId(targetId);
      openWorkspacePathMutation.mutate({ path, targetId });
    },
    [openWorkspacePathMutation],
  );

  const value = useMemo(
    () =>
      workspace
        ? {
            workspaceRootPath: workspace.worktreePath,
            targets,
            preferredTargetId,
            openingPath: opening?.path ?? null,
            openingTargetId: opening?.targetId ?? null,
            onOpenPath: openPath,
          }
        : null,
    [openPath, opening?.path, opening?.targetId, preferredTargetId, targets, workspace],
  );

  // Always the same element type at this position. Returning a bare fragment
  // for the no-workspace case swapped the type whenever a workspace appeared or
  // disappeared, and React tears down and rebuilds the whole chat subtree on
  // that swap: a fresh transcript mounts pinned at the bottom, so a reader
  // parked mid-history was yanked down every time a run completed and the
  // workspace query re-resolved. Consumers already treat a null value as "no
  // workspace", so the two branches were never semantically different.
  return (
    <MessageFileLinkContext.Provider value={value}>
      {children}
    </MessageFileLinkContext.Provider>
  );
}
