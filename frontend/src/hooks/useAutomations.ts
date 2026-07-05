import { useQuery } from "@tanstack/react-query";

import { automationsApi } from "@/api/automations";

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
