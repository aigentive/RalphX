import { QueryClientProvider } from "@tanstack/react-query";
import { useEffect, type ReactNode } from "react";

import { getQueryClient } from "@/lib/queryClient";
import { useEnvironmentStore } from "@/stores/environmentStore";
import { EventProvider } from "./EventProvider";

interface EnvironmentScopedProvidersProps {
  children: ReactNode;
}

export function EnvironmentScopedProviders({
  children,
}: EnvironmentScopedProvidersProps) {
  const activeEnvironmentId = useEnvironmentStore(
    (state) => state.activeEnvironmentId,
  );
  const queryClient = getQueryClient(activeEnvironmentId);

  useEffect(() => {
    // Single writer after initial client creation: Playwright always observes
    // the client belonging to the provider subtree that is currently mounted.
    if (typeof window !== "undefined" && !window.__TAURI_INTERNALS__) {
      window.__queryClient = queryClient;
    }
  }, [queryClient]);

  return (
    <QueryClientProvider key={activeEnvironmentId} client={queryClient}>
      <EventProvider environmentId={activeEnvironmentId}>
        {children}
      </EventProvider>
    </QueryClientProvider>
  );
}
