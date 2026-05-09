import { describe, expect, it } from "vitest";

import {
  hydrateRalphxRuntimeEnvFromCli,
  parseCliOptionFromArgs,
} from "../runtime-context.js";

describe("parseCliOptionFromArgs", () => {
  it("supports inline and pair-style CLI options", () => {
    expect(
      parseCliOptionFromArgs(
        ["node", "index.js", "--context-type=ideation"],
        "context-type"
      )
    ).toBe("ideation");

    expect(
      parseCliOptionFromArgs(
        ["node", "index.js", "--context-id", "session-123"],
        "context-id"
      )
    ).toBe("session-123");
  });
});

describe("hydrateRalphxRuntimeEnvFromCli", () => {
  it("hydrates process-style env values from RalphX CLI args", () => {
    const env: NodeJS.ProcessEnv = {};

    const runtimeContext = hydrateRalphxRuntimeEnvFromCli(
      [
        "node",
        "index.js",
        "--agent-type",
        "ralphx-plan-verifier",
        "--context-type",
        "ideation",
        "--context-id",
        "session-123",
        "--project-id",
        "project-456",
        "--working-directory",
        "/tmp/workspace",
        "--tauri-api-url",
        "http://127.0.0.1:3857",
      ],
      env
    );

    expect(runtimeContext.agentType).toBe("ralphx-plan-verifier");
    expect(runtimeContext.contextType).toBe("ideation");
    expect(runtimeContext.contextId).toBe("session-123");
    expect(runtimeContext.projectId).toBe("project-456");
    expect(runtimeContext.workingDirectory).toBe("/tmp/workspace");
    expect(runtimeContext.tauriApiUrl).toBe("http://127.0.0.1:3857");
    expect(env.RALPHX_AGENT_TYPE).toBe("ralphx-plan-verifier");
    expect(env.RALPHX_CONTEXT_TYPE).toBe("ideation");
    expect(env.RALPHX_CONTEXT_ID).toBe("session-123");
    expect(env.RALPHX_PROJECT_ID).toBe("project-456");
    expect(env.RALPHX_WORKING_DIRECTORY).toBe("/tmp/workspace");
    expect(env.TAURI_API_URL).toBe("http://127.0.0.1:3857");
  });
});
