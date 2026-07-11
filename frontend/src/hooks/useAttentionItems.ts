import { useQuery } from "@tanstack/react-query";

import { notificationsApi } from "@/api/notifications";

export const attentionKeys = {
  all: ["attention"] as const,
  list: (projectId?: string) => [...attentionKeys.all, "list", projectId ?? "all"] as const,
};

export function useAttentionItems(projectId?: string, options: { enabled?: boolean } = {}) {
  return useQuery({
    queryKey: attentionKeys.list(projectId),
    queryFn: () => notificationsApi.listAttentionItems(projectId),
    staleTime: 30_000,
    enabled: options.enabled ?? true,
    // A failed refresh must retain the last authoritative count rather than zeroing the badge.
    placeholderData: (previousData) => previousData,
  });
}
