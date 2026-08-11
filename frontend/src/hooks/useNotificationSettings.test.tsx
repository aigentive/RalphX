import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { useNotificationSettings } from "./useNotificationSettings";

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>{children}</QueryClientProvider>;
}

describe("useNotificationSettings", () => {
  beforeEach(() => invokeMock.mockReset());

  it("parses nullable notification settings fields into tolerant defaults", async () => {
    invokeMock.mockResolvedValue({
      desktop_enabled: null,
      desktop_only_when_unfocused: true,
      focused_toasts_enabled: null,
      desktop_agent_requests_enabled: true,
      desktop_agent_waiting_enabled: true,
      desktop_reviews_enabled: true,
      desktop_task_failures_enabled: true,
      desktop_automation_approvals_enabled: true,
      desktop_automation_run_completions_enabled: false,
      desktop_git_github_enabled: true,
      muted_project_ids: null,
    });

    const { result } = renderHook(() => useNotificationSettings(), { wrapper });

    await waitFor(() => expect(result.current.data).toMatchObject({
      desktop_enabled: true,
      focused_toasts_enabled: true,
      muted_project_ids: [],
    }));
  });
});
