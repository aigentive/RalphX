import { createElement, type ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const getAttributions = vi.hoisted(() => vi.fn());
vi.mock("@/api/agent-runs", () => ({ agentRunsApi: { getAttributions } }));

import { useRunAttributions } from "./useRunAttributions";

afterEach(() => {
  getAttributions.mockReset();
});

function wrapper({ children }: { children: ReactNode }) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return createElement(QueryClientProvider, { client: queryClient }, children);
}

describe("useRunAttributions", () => {
  it("sorts and deduplicates ids, then batches the backend calls at 100", async () => {
    const runIds = Array.from({ length: 101 }, (_, index) => `run-${String(index).padStart(3, "0")}`);
    getAttributions.mockImplementation(async (ids: string[]) => ids.map((id) => ({ id })));

    const { result } = renderHook(
      () => useRunAttributions(["run-100", "run-000", "run-000", ...runIds.slice(1, 100)]),
      { wrapper },
    );

    await waitFor(() => expect(result.current.data).toBeInstanceOf(Map));
    expect(getAttributions).toHaveBeenCalledTimes(2);
    expect(getAttributions).toHaveBeenNthCalledWith(1, runIds.slice(0, 100));
    expect(getAttributions).toHaveBeenNthCalledWith(2, ["run-100"]);
    expect(result.current.data?.get("run-100")).toEqual({ id: "run-100" });
  });

  it("does not fetch before transcript readiness, then fetches after reveal", async () => {
    getAttributions.mockResolvedValue([]);
    const { rerender } = renderHook(
      ({ enabled }) => useRunAttributions(["run-1"], { enabled }),
      { initialProps: { enabled: false }, wrapper },
    );
    expect(getAttributions).not.toHaveBeenCalled();

    rerender({ enabled: true });
    await waitFor(() => expect(getAttributions).toHaveBeenCalledWith(["run-1"]));
  });
});
