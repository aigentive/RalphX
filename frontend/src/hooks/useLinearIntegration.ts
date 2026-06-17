import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  linearApi,
  type SaveLinearWebhookSigningSecretInput,
} from "@/api/linear";

export const linearIntegrationKeys = {
  all: ["linear-integration"] as const,
  webhookConfig: () => [...linearIntegrationKeys.all, "webhook-config"] as const,
};

export function useLinearIntegration() {
  const queryClient = useQueryClient();
  const webhookConfigQuery = useQuery({
    queryKey: linearIntegrationKeys.webhookConfig(),
    queryFn: () => linearApi.getWebhookConfig(),
    staleTime: 15_000,
  });

  const saveWebhookSecretMutation = useMutation({
    mutationFn: (input: SaveLinearWebhookSigningSecretInput) =>
      linearApi.saveWebhookSigningSecret(input),
    onSuccess: (config) => {
      queryClient.setQueryData(linearIntegrationKeys.webhookConfig(), config);
    },
  });

  return {
    webhookConfig: webhookConfigQuery.data,
    isLoading: webhookConfigQuery.isLoading,
    isError: webhookConfigQuery.isError,
    error: webhookConfigQuery.error,
    saveWebhookSigningSecretAsync: saveWebhookSecretMutation.mutateAsync,
    isSavingWebhookSigningSecret: saveWebhookSecretMutation.isPending,
    saveWebhookSigningSecretError: saveWebhookSecretMutation.error,
  };
}
