import {
  useCallback,
  useEffect,
  useMemo,
  useState,
} from "react";

import type { AgentSidebarConversationRow } from "@/api/chat";

import {
  isBulkArchiveConversationEligible,
  toBulkArchiveConversationTarget,
  type BulkArchiveConversationHandler,
  type BulkArchiveConversationTarget,
} from "./bulkConversationArchive";
import {
  useBulkArchiveSelection,
  type BulkArchiveSelectionContextValue,
} from "./bulkConversationArchiveSelectionContext";

interface BulkConversationArchiveController {
  active: boolean;
  confirmationOpen: boolean;
  contextValue: BulkArchiveSelectionContextValue;
  enter: () => void;
  cancel: () => void;
  closeConfirmation: () => void;
  confirm: () => Promise<void>;
  openConfirmation: () => void;
  pending: boolean;
  selectedCount: number;
}

export function useRegisterBulkArchiveRows(
  sourceKey: string,
  rows: AgentSidebarConversationRow[]
) {
  const { registerLoadedTargets, unregisterLoadedTargets } =
    useBulkArchiveSelection();
  const targets = useMemo(
    () => rows.map(toBulkArchiveConversationTarget),
    [rows]
  );

  useEffect(() => {
    registerLoadedTargets(sourceKey, targets);
  }, [registerLoadedTargets, sourceKey, targets]);

  useEffect(
    () => () => unregisterLoadedTargets(sourceKey),
    [sourceKey, unregisterLoadedTargets]
  );
}

export function useBulkConversationArchiveController(
  onBulkArchiveConversations: BulkArchiveConversationHandler
): BulkConversationArchiveController {
  const [loadedTargetsBySource, setLoadedTargetsBySource] = useState(
    () => new Map<string, BulkArchiveConversationTarget[]>()
  );
  const [active, setActive] = useState(false);
  const [confirmationOpen, setConfirmationOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const [selectedTargetsById, setSelectedTargetsById] = useState<
    Record<string, BulkArchiveConversationTarget>
  >({});

  const registerLoadedTargets = useCallback(
    (sourceKey: string, targets: BulkArchiveConversationTarget[]) => {
      setLoadedTargetsBySource((current) => {
        if (areBulkArchiveTargetListsEqual(current.get(sourceKey), targets)) {
          return current;
        }
        const next = new Map(current);
        next.set(sourceKey, targets);
        return next;
      });
    },
    []
  );
  const unregisterLoadedTargets = useCallback((sourceKey: string) => {
    setLoadedTargetsBySource((current) => {
      if (!current.has(sourceKey)) {
        return current;
      }
      const next = new Map(current);
      next.delete(sourceKey);
      return next;
    });
  }, []);
  const loadedTargetsById = useMemo(() => {
    const targetsById = new Map<string, BulkArchiveConversationTarget>();
    for (const targets of loadedTargetsBySource.values()) {
      for (const target of targets) {
        targetsById.set(target.conversation.id, target);
      }
    }
    return targetsById;
  }, [loadedTargetsBySource]);

  useEffect(() => {
    if (!active) {
      return;
    }
    setSelectedTargetsById((current) => {
      const next: Record<string, BulkArchiveConversationTarget> = {};
      let changed = false;
      for (const conversationId of Object.keys(current)) {
        const loadedTarget = loadedTargetsById.get(conversationId);
        if (!loadedTarget || !isBulkArchiveConversationEligible(loadedTarget)) {
          changed = true;
          continue;
        }
        next[conversationId] = loadedTarget;
        changed = changed || current[conversationId] !== loadedTarget;
      }
      return changed ? next : current;
    });
  }, [active, loadedTargetsById]);

  const enter = useCallback(() => {
    setActive(true);
  }, []);
  const cancel = useCallback(() => {
    if (pending) {
      return;
    }
    setConfirmationOpen(false);
    setSelectedTargetsById({});
    setActive(false);
  }, [pending]);
  const toggleTarget = useCallback((target: BulkArchiveConversationTarget) => {
    const conversationId = target.conversation.id;
    const loadedTarget = findLoadedTarget(
      loadedTargetsBySource,
      conversationId
    );
    if (!loadedTarget || !isBulkArchiveConversationEligible(loadedTarget)) {
      return;
    }
    setSelectedTargetsById((current) => {
      if (current[conversationId]) {
        const next = { ...current };
        delete next[conversationId];
        return next;
      }
      return { ...current, [conversationId]: loadedTarget };
    });
  }, [loadedTargetsBySource]);
  const selectedIds = useMemo(
    () => new Set(Object.keys(selectedTargetsById)),
    [selectedTargetsById]
  );
  const selectedCount = selectedIds.size;
  const openConfirmation = useCallback(() => {
    if (selectedCount > 0 && !pending) {
      setConfirmationOpen(true);
    }
  }, [pending, selectedCount]);
  const closeConfirmation = useCallback(() => {
    if (!pending) {
      setConfirmationOpen(false);
    }
  }, [pending]);
  const confirm = useCallback(async () => {
    if (pending) {
      return;
    }
    const currentTargets = Object.keys(selectedTargetsById)
      .map((conversationId) =>
        findLoadedTarget(loadedTargetsBySource, conversationId)
      )
      .filter(
        (target): target is BulkArchiveConversationTarget =>
          target !== null && isBulkArchiveConversationEligible(target)
      );
    if (currentTargets.length === 0) {
      setSelectedTargetsById({});
      setConfirmationOpen(false);
      return;
    }

    setPending(true);
    try {
      const result = await onBulkArchiveConversations(currentTargets);
      if (result.failedConversationIds.length === 0) {
        setSelectedTargetsById({});
        setConfirmationOpen(false);
        setActive(false);
        return;
      }

      const failedIds = new Set(result.failedConversationIds);
      const failedTargets: Record<string, BulkArchiveConversationTarget> = {};
      for (const conversationId of failedIds) {
        const loadedTarget = findLoadedTarget(
          loadedTargetsBySource,
          conversationId
        );
        if (loadedTarget && isBulkArchiveConversationEligible(loadedTarget)) {
          failedTargets[conversationId] = loadedTarget;
        }
      }
      setSelectedTargetsById(failedTargets);
    } finally {
      setPending(false);
    }
  }, [
    loadedTargetsBySource,
    onBulkArchiveConversations,
    pending,
    selectedTargetsById,
  ]);

  const contextValue = useMemo<BulkArchiveSelectionContextValue>(
    () => ({
      active,
      pending,
      registerLoadedTargets,
      selectedIds,
      toggleTarget,
      unregisterLoadedTargets,
    }),
    [
      active,
      pending,
      registerLoadedTargets,
      selectedIds,
      toggleTarget,
      unregisterLoadedTargets,
    ]
  );

  return {
    active,
    cancel,
    closeConfirmation,
    confirmationOpen,
    confirm,
    contextValue,
    enter,
    openConfirmation,
    pending,
    selectedCount,
  };
}

function areBulkArchiveTargetListsEqual(
  current: BulkArchiveConversationTarget[] | undefined,
  next: BulkArchiveConversationTarget[]
) {
  if (!current || current.length !== next.length) {
    return false;
  }
  return current.every((target, index) => {
    const nextTarget = next[index];
    if (!nextTarget) {
      return false;
    }
    return (
      target.conversation.id === nextTarget.conversation.id &&
      target.conversation.updatedAt === nextTarget.conversation.updatedAt &&
      target.conversation.archivedAt === nextTarget.conversation.archivedAt &&
      target.workspace?.linkedPlanBranchId ===
        nextTarget.workspace?.linkedPlanBranchId &&
      target.workspace?.publicationPrNumber ===
        nextTarget.workspace?.publicationPrNumber &&
      target.workspace?.publicationPrStatus ===
        nextTarget.workspace?.publicationPrStatus
    );
  });
}

function findLoadedTarget(
  targetsBySource: Map<string, BulkArchiveConversationTarget[]>,
  conversationId: string
): BulkArchiveConversationTarget | null {
  for (const targets of targetsBySource.values()) {
    const target = targets.find(
      (candidate) => candidate.conversation.id === conversationId
    );
    if (target) {
      return target;
    }
  }
  return null;
}
