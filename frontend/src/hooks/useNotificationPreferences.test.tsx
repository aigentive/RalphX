import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { useNotificationPreferences } from "./useNotificationPreferences";

describe("useNotificationPreferences", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("uses persisted focused_toasts_enabled after the settings query resolves", async () => {
    invokeMock.mockResolvedValue({ focused_toasts_enabled: false });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    const { result } = renderHook(() => useNotificationPreferences(), { wrapper });

    expect(result.current).toMatchObject({ ready: false, focusedToastsEnabled: false });
    await waitFor(() => expect(result.current).toMatchObject({
      ready: true,
      focusedToastsEnabled: false,
    }));
    expect(invokeMock).toHaveBeenCalledWith("get_notification_settings", {});
  });
});
