import { useEffect } from "react";
import { type QueryClient, useQuery, useQueryClient } from "@tanstack/react-query";

import { automationsApi } from "@/api/automations";
import { useEventBus } from "@/providers/EventProvider";

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
  run_id?: string | null;
  runId?: string | null;
}

function payloadAutomationId(payload: AutomationEventPayload): string | null {
  return payload.automationId ?? payload.automation_id ?? null;
}

export function invalidateAutomationQueries(
  queryClient: QueryClient,
  automationId?: string | null,
) {
  void queryClient.invalidateQueries({ queryKey: automationKeys.lists() });
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
        invalidateAutomationQueries(queryClient, eventAutomationId ?? automationId ?? null);
      },
    );

    return () => {
      unsubscribeAutomation();
      unsubscribeRun();
    };
  }, [automationId, bus, queryClient]);
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
