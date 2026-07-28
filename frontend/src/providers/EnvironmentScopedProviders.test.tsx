import { invoke } from "@tauri-apps/api/core";
import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { getQueryClient, resetQueryClient } from "@/lib/queryClient";
import { resetTransportEnvironmentId } from "@/lib/remote/active-environment";
import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";
import { EnvironmentScopedProviders } from "./EnvironmentScopedProviders";

vi.mock("@/api/remote-environments", () => ({
  remoteEnvironmentsApi: {
    pair: vi.fn(),
    list: vi.fn(),
    remove: vi.fn(),
    getActiveEnvironment: vi.fn(),
    setActiveEnvironment: vi.fn(),
  },
}));

vi.mock("@/providers/EventProvider", () => ({
  EventProvider: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

function ClientProbe({
  onClient,
  onMount,
  onUnmount,
}: {
  onClient?: (client: QueryClient) => void;
  onMount?: () => void;
  onUnmount?: () => void;
}) {
  const client = useQueryClient();
  onClient?.(client);

  useEffect(() => {
    onMount?.();
    return () => onUnmount?.();
  }, [onMount, onUnmount]);

  return (
    <div data-testid="client-probe">
      {client.getQueryData(["retained"]) ?? "empty"}
    </div>
  );
}

function setActiveEnvironmentId(activeEnvironmentId: string): void {
  act(() => {
    useEnvironmentStore.setState({ activeEnvironmentId });
  });
}

beforeEach(() => {
  resetQueryClient();
  resetTransportEnvironmentId();
  useEnvironmentStore.setState({ activeEnvironmentId: LOCAL_ENVIRONMENT_ID });
  window.__queryClient = undefined;
});

afterEach(() => {
  resetQueryClient();
  resetTransportEnvironmentId();
  useEnvironmentStore.setState({ activeEnvironmentId: LOCAL_ENVIRONMENT_ID });
  vi.unstubAllGlobals();
});

describe("EnvironmentScopedProviders", () => {
  it("provides the active environment's QueryClient", () => {
    const observedClients: QueryClient[] = [];
    render(
      <EnvironmentScopedProviders>
        <ClientProbe onClient={(client) => observedClients.push(client)} />
      </EnvironmentScopedProviders>,
    );

    expect(observedClients).toEqual([getQueryClient(LOCAL_ENVIRONMENT_ID)]);
  });

  it("remounts the subtree with the new environment client", () => {
    const mount = vi.fn();
    const unmount = vi.fn();
    render(
      <EnvironmentScopedProviders>
        <ClientProbe onMount={mount} onUnmount={unmount} />
      </EnvironmentScopedProviders>,
    );
    const localClient = window.__queryClient;

    setActiveEnvironmentId("env-b");

    expect(mount).toHaveBeenCalledTimes(2);
    expect(unmount).toHaveBeenCalledTimes(1);
    expect(window.__queryClient).toBe(getQueryClient("env-b"));
    expect(window.__queryClient).not.toBe(localClient);
  });

  it("retains the original cache when switching away and back", () => {
    const localClient = getQueryClient(LOCAL_ENVIRONMENT_ID);
    localClient.setQueryData(["retained"], "local-value");
    render(
      <EnvironmentScopedProviders>
        <ClientProbe />
      </EnvironmentScopedProviders>,
    );

    setActiveEnvironmentId("env-b");
    setActiveEnvironmentId(LOCAL_ENVIRONMENT_ID);

    expect(window.__queryClient).toBe(localClient);
    expect(screen.getByTestId("client-probe")).toHaveTextContent("local-value");
    expect(localClient.getQueryData(["retained"])).toBe("local-value");
  });

  it("performs no fetch or invoke when it mounts and remounts", () => {
    const fetchSpy = vi.fn<typeof fetch>();
    vi.stubGlobal("fetch", fetchSpy);

    render(
      <EnvironmentScopedProviders>
        <ClientProbe />
      </EnvironmentScopedProviders>,
    );
    setActiveEnvironmentId("env-b");

    expect(fetchSpy).not.toHaveBeenCalled();
    expect(vi.mocked(invoke)).not.toHaveBeenCalled();
  });

  it("keeps the web-mode Playwright client aligned across a switch", async () => {
    render(
      <EnvironmentScopedProviders>
        <ClientProbe />
      </EnvironmentScopedProviders>,
    );
    expect(window.__queryClient).toBe(getQueryClient(LOCAL_ENVIRONMENT_ID));

    setActiveEnvironmentId("env-b");

    await waitFor(() => {
      expect(window.__queryClient).toBe(getQueryClient("env-b"));
    });
  });
});
