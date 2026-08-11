/**
 * Test utilities for Zustand stores and React Query hooks
 *
 * Provides helpers for testing components and hooks that depend on
 * the application's state management infrastructure.
 */

import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import { useTaskStore } from "@/stores/taskStore";
import { useProjectStore } from "@/stores/projectStore";
import { useActivityStore } from "@/stores/activityStore";
import { useUiStore } from "@/stores/uiStore";

/**
 * Create a test QueryClient with sensible defaults
 *
 * Disables retries and other behaviors that can cause flaky tests.
 *
 * @returns A new QueryClient configured for testing
 *
 * @example
 * ```tsx
 * const queryClient = createTestQueryClient();
 * render(<QueryClientProvider client={queryClient}><MyComponent /></QueryClientProvider>);
 * ```
 */
export function createTestQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        gcTime: 0,
        staleTime: 0,
      },
      mutations: {
        retry: false,
      },
    },
  });
}

/**
 * Create a wrapper component for testing hooks
 *
 * @param queryClient - Optional QueryClient to use (creates new one if not provided)
 * @returns A wrapper component that provides QueryClientProvider
 *
 * @example
 * ```tsx
 * const wrapper = createWrapper();
 * const { result } = renderHook(() => useTasks("project-1"), { wrapper });
 * ```
 */
export function createWrapper(queryClient?: QueryClient) {
  const client = queryClient ?? createTestQueryClient();

  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
  };
}

/**
 * Render a hook with QueryClientProvider wrapper
 *
 * Convenience function that combines renderHook with createWrapper.
 *
 * @param hook - The hook function to render
 * @param queryClient - Optional QueryClient to use
 * @returns The result from renderHook
 *
 * @example
 * ```tsx
 * const { result } = renderHookWithProviders(() => useTasks("project-1"));
 * await waitFor(() => expect(result.current.isSuccess).toBe(true));
 * ```
 */
export function renderHookWithProviders<T>(
  hook: () => T,
  queryClient?: QueryClient
) {
  const wrapper = createWrapper(queryClient);
  return renderHook(hook, { wrapper });
}

/**
 * Reset all Zustand stores to their initial state
 *
 * Call this in beforeEach to ensure clean state between tests.
 *
 * @example
 * ```tsx
 * beforeEach(() => {
 *   resetAllStores();
 * });
 * ```
 */
export function resetAllStores(): void {
  useTaskStore.setState({
    tasks: {},
  });

  useProjectStore.setState({
    projects: {},
    activeProjectId: null,
  });

  useActivityStore.setState({
    messages: [],
    alerts: [],
  });

  useUiStore.setState({
    sidebarOpen: true,
    activeModal: null,
    modalContext: undefined,
    notifications: [],
    loading: {},
    confirmation: null,
  });
}

/**
 * Reset a specific store to its initial state
 *
 * @param storeName - The name of the store to reset
 *
 * @example
 * ```tsx
 * resetStore("task");
 * ```
 */
export function resetStore(
  storeName: "task" | "project" | "activity" | "ui"
): void {
  switch (storeName) {
    case "task":
      useTaskStore.setState({ tasks: {} });
      break;
    case "project":
      useProjectStore.setState({ projects: {}, activeProjectId: null });
      break;
    case "activity":
      useActivityStore.setState({ messages: [], alerts: [] });
      break;
    case "ui":
      useUiStore.setState({
        sidebarOpen: true,
        activeModal: null,
        modalContext: undefined,
        notifications: [],
        loading: {},
        confirmation: null,
      });
      break;
  }
}
