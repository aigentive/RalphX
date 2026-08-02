import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import type { ReactNode } from "react";

import { TooltipProvider } from "@/components/ui/tooltip";
import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";

const REMOTE_ENVIRONMENT_ID = "remote-detail-view";

export type DetailViewEnvironment = "remote-default" | "remote-agent" | "local";

export function setDetailViewEnvironment(environment: DetailViewEnvironment): void {
  if (environment === "local") {
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
      effectiveScopes: {},
      connectionPresentations: {},
    });
    return;
  }

  useEnvironmentStore.setState({
    activeEnvironmentId: REMOTE_ENVIRONMENT_ID,
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      { id: REMOTE_ENVIRONMENT_ID, name: "Studio Mac", kind: "remote" },
    ],
    effectiveScopes: {
      [REMOTE_ENVIRONMENT_ID]: environment === "remote-agent"
        ? ["ui:read", "ui:operate", "ui:agent"]
        : ["ui:read", "ui:operate"],
    },
    connectionPresentations: {
      [REMOTE_ENVIRONMENT_ID]: {
        presentation: "connected",
        blockedFailure: null,
        blockedMessage: null,
      },
    },
  });
}

export function renderGatedDetailView(
  node: ReactNode,
  environment: DetailViewEnvironment,
): { queryClient: QueryClient; unmount: () => void } {
  setDetailViewEnvironment(environment);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const result = render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>{node}</TooltipProvider>
    </QueryClientProvider>,
  );
  return { queryClient, unmount: result.unmount };
}
