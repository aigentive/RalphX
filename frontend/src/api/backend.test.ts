import { afterEach, describe, expect, it, vi } from "vitest";

import { backendApiUrl, backendBaseUrl } from "./backend";

describe("backend API URL helpers", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("ignores live dev backend URL overrides during unit tests", () => {
    vi.stubEnv("VITE_RALPHX_BACKEND_URL", "http://127.0.0.1:3857");

    expect(backendBaseUrl()).toBe("http://localhost:3847");
    expect(backendApiUrl("agent_tasks/list")).toBe(
      "http://localhost:3847/api/agent_tasks/list",
    );
  });
});
