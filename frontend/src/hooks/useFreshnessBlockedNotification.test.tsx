import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";

const { subscribers, getTask, toastWarning } = vi.hoisted(() => ({
  subscribers: new Map<string, (payload: unknown) => Promise<void>>(),
  getTask: vi.fn(),
  toastWarning: vi.fn(),
}));

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: (event: string, callback: (payload: unknown) => Promise<void>) => {
      subscribers.set(event, callback);
      return vi.fn();
    },
  }),
}));
vi.mock("@/lib/tauri", () => ({ api: { tasks: { get: getTask, move: vi.fn() } } }));
vi.mock("@/hooks/useTasks", () => ({ taskKeys: { all: ["tasks"] } }));
vi.mock("@/hooks/useExecutionControl", () => ({ executionKeys: { all: ["execution"] } }));
vi.mock("@/lib/task-actions/resume-execution-if-stopped", () => ({ resumeExecutionIfStopped: vi.fn() }));
vi.mock("sonner", () => ({ toast: { warning: toastWarning, error: vi.fn() } }));

import { useFreshnessBlockedNotification } from "./useFreshnessBlockedNotification";

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={new QueryClient()}>{children}</QueryClientProvider>;
}

describe("useFreshnessBlockedNotification", () => {
  beforeEach(() => {
    subscribers.clear();
    toastWarning.mockReset();
    getTask.mockResolvedValue({
      id: "task-1",
      projectId: "project-1",
      title: "Resolve branch conflicts",
      blockedReason: "FRESHNESS_BLOCKED|3|10|src/lib.rs|Persistent freshness conflicts",
    });
    renderHook(() => useFreshnessBlockedNotification(), { wrapper });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("emits exactly one specialized reset toast for a freshness-blocked event", async () => {
    const callback = subscribers.get("task:status_changed");
    expect(callback).toBeDefined();

    await callback?.({ task_id: "task-1", old_status: "re_executing", new_status: "blocked" });
    await callback?.({ task_id: "task-1", old_status: "re_executing", new_status: "blocked" });

    expect(toastWarning).toHaveBeenCalledTimes(1);
    expect(toastWarning).toHaveBeenCalledWith(
      "Branch freshness blocked — Resolve branch conflicts",
      expect.objectContaining({ action: expect.objectContaining({ label: "Reset & Retry" }) }),
    );
  });
});
