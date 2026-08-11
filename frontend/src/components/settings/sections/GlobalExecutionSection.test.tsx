import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { executionApi } from "@/api/execution";
import GlobalExecutionSection from "./GlobalExecutionSection";

vi.mock("@/api/execution", () => ({
  executionApi: {
    getGlobalSettings: vi.fn(),
    updateGlobalSettings: vi.fn(),
  },
}));

vi.mock("sonner", () => ({
  toast: { error: vi.fn() },
}));

const settings = {
  globalMaxConcurrent: 20,
  workspaceMaxConcurrent: 10,
  globalIdeationMax: 10,
  allowIdeationBorrowIdleExecution: false,
};

describe("GlobalExecutionSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(executionApi.getGlobalSettings).mockResolvedValue(settings);
    vi.mocked(executionApi.updateGlobalSettings).mockResolvedValue(undefined);
  });

  it("flushes a pending capacity edit when the section unmounts", async () => {
    const { unmount } = render(<GlobalExecutionSection embedded />);
    const input = await screen.findByTestId("global-max-concurrent");

    fireEvent.change(input, { target: { value: "24" } });
    unmount();

    await waitFor(() => {
      expect(executionApi.updateGlobalSettings).toHaveBeenCalledWith({
        ...settings,
        globalMaxConcurrent: 24,
      });
    });
  });
});
