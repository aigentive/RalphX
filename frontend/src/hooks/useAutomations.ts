import { useEffect } from "react";
import {
  type QueryClient,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { automationsApi } from "@/api/automations";
import type {
  CreateAutomationDraftInput,
  CreateAutomationDraftResponse,
} from "@/api/automations";
import { useEventBus } from "@/providers/EventProvider";

import { agentSidebarConversationKeys } from "./agentSidebarConversationKeys";

export const automationKeys = {
  all: ["automations"] as const,
  lists: () => [...automationKeys.all, "list"] as const,
  list: (projectId: string | null | undefined) =>
    [...automationKeys.lists(), projectId ?? "none"] as const,
  details: () => [...automationKeys.all, "detail"] as const,
  detail: (id: string | null | undefined) =>
    [...automationKeys.details(), id ?? "none"] as const,
};

export function useAutomationsList(
  projectId: string | null | undefined,
  options: { enabled?: boolean } = {},
) {
  return useQuery({
    queryKey: automationKeys.list(projectId),
    queryFn: () => automationsApi.list({ projectId }),
    enabled: Boolean(projectId) && (options.enabled ?? true),
    staleTime: 5_000,
  });
}

interface AutomationEventPayload {
  automation_id?: string | null;
  automationId?: string | null;
  project_id?: string | null;
  projectId?: string | null;
  run_id?: string | null;
  runId?: string | null;
}

function payloadAutomationId(payload: AutomationEventPayload): string | null {
  return payload.automationId ?? payload.automation_id ?? null;
}

/**
 * Handle an `automation:deleted` event: the automation row is gone, so refresh
 * the lists and evict its detail query rather than invalidating it (which would
 * trigger a doomed 404 refetch).
 */
export function evictDeletedAutomation(
  queryClient: QueryClient,
  automationId?: string | null,
) {
  void queryClient.invalidateQueries({ queryKey: automationKeys.lists() });
  void queryClient.invalidateQueries({
    queryKey: agentSidebarConversationKeys.automationScope(),
  });
  if (automationId) {
    queryClient.removeQueries({
      queryKey: automationKeys.detail(automationId),
    });
  }
}

export function invalidateAutomationQueries(
  queryClient: QueryClient,
  automationId?: string | null,
) {
  void queryClient.invalidateQueries({ queryKey: automationKeys.lists() });
  void queryClient.invalidateQueries({
    queryKey: agentSidebarConversationKeys.automationScope(),
  });
  if (automationId) {
    void queryClient.invalidateQueries({
      queryKey: automationKeys.detail(automationId),
    });
  } else {
    void queryClient.invalidateQueries({
      queryKey: automationKeys.details(),
    });
  }
}

export function invalidateAutomationRunQueries(
  queryClient: QueryClient,
  automationId?: string | null,
) {
  if (!automationId) {
    return;
  }
  void queryClient.invalidateQueries({
    queryKey: automationKeys.detail(automationId),
  });
}

export function useAutomationEvents(automationId?: string | null) {
  const bus = useEventBus();
  const queryClient = useQueryClient();

  useEffect(() => {
    const unsubscribeAutomation = bus.subscribe<AutomationEventPayload>(
      "automation:updated",
      (payload) => {
        const eventAutomationId = payloadAutomationId(payload);
        if (automationId && eventAutomationId && eventAutomationId !== automationId) {
          return;
        }
        invalidateAutomationQueries(queryClient, eventAutomationId ?? automationId ?? null);
      },
    );
    const unsubscribeRun = bus.subscribe<AutomationEventPayload>(
      "automation:run:updated",
      (payload) => {
        const eventAutomationId = payloadAutomationId(payload);
        if (automationId && eventAutomationId && eventAutomationId !== automationId) {
          return;
        }
        invalidateAutomationRunQueries(
          queryClient,
          eventAutomationId ?? automationId ?? null,
        );
      },
    );
    const unsubscribeDeleted = bus.subscribe<AutomationEventPayload>(
      "automation:deleted",
      (payload) => {
        const eventAutomationId = payloadAutomationId(payload);
        if (automationId && eventAutomationId && eventAutomationId !== automationId) {
          return;
        }
        evictDeletedAutomation(queryClient, eventAutomationId ?? automationId ?? null);
      },
    );

    return () => {
      unsubscribeAutomation();
      unsubscribeRun();
      unsubscribeDeleted();
    };
  }, [automationId, bus, queryClient]);
}

export function useCreateAutomationDraft() {
  const queryClient = useQueryClient();
  return useMutation<
    CreateAutomationDraftResponse,
    Error,
    CreateAutomationDraftInput
  >({
    mutationFn: (input) => automationsApi.createDraft(input),
    onSuccess: (result) => {
      invalidateAutomationQueries(queryClient, result.automation.id);
    },
  });
}

export function useAutomationDetail(
  id: string | null | undefined,
  options: { enabled?: boolean } = {},
) {
  return useQuery({
    queryKey: automationKeys.detail(id),
    queryFn: () => automationsApi.get(id ?? ""),
    enabled: Boolean(id) && (options.enabled ?? true),
    staleTime: 5_000,
  });
}
