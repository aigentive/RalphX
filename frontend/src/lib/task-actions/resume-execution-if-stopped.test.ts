import { beforeEach, describe, expect, it, vi } from "vitest";

import { api } from "@/lib/tauri";
import { resumeExecutionIfStopped } from "./resume-execution-if-stopped";

vi.mock("@/lib/tauri", () => ({
  api: {
    execution: {
      getStatus: vi.fn(),
      resume: vi.fn(),
    },
  },
}));

describe("resumeExecutionIfStopped", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("surfaces an unreadable execution status without attempting resume", async () => {
    vi.mocked(api.execution.getStatus).mockRejectedValue(new Error("host status unavailable"));

    await expect(resumeExecutionIfStopped("project-1")).rejects.toThrow(
      "Unable to read execution status: host status unavailable"
    );
    expect(api.execution.resume).not.toHaveBeenCalled();
  });
});
