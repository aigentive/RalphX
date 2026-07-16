import { describe, expect, it } from "vitest";

import {
  buildArtifactMutationTransportHeaders,
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
        "ralphx-ideation",
        "--agent-profile",
        "plan",
        "--context-type",
        "ideation",
        "--context-id",
        "session-123",
        "--conversation-id",
        "conversation-current",
        "--coordination-mode",
        "rx_native_workflow",
        "--parent-conversation-id",
        "conversation-789",
        "--agent-run-id",
        "run-current",
        "--task-state",
        "re_executing",
        "--project-id",
        "project-456",
        "--working-directory",
        "/tmp/workspace",
        "--filesystem-read-root",
        "/tmp/project",
        "--filesystem-read-root=/tmp/shared-artifacts",
        "--tauri-api-url",
        "http://127.0.0.1:3857",
        "--trace-dir",
        "/tmp/ralphx-logs/mcp-proxy",
      ],
      env
    );

    expect(runtimeContext.agentType).toBe("ralphx-ideation");
    expect(runtimeContext.agentProfile).toBe("plan");
    expect(runtimeContext.contextType).toBe("ideation");
    expect(runtimeContext.contextId).toBe("session-123");
    expect(runtimeContext.conversationId).toBe("conversation-current");
    expect(runtimeContext.coordinationMode).toBe("rx_native_workflow");
    expect(runtimeContext.parentConversationId).toBe("conversation-789");
    expect(runtimeContext.agentRunId).toBe("run-current");
    expect(runtimeContext.taskState).toBe("re_executing");
    expect(runtimeContext.projectId).toBe("project-456");
    expect(runtimeContext.workingDirectory).toBe("/tmp/workspace");
    expect(runtimeContext.filesystemReadRoots).toBe(
      JSON.stringify(["/tmp/project", "/tmp/shared-artifacts"])
    );
    expect(runtimeContext.tauriApiUrl).toBe("http://127.0.0.1:3857");
    expect(runtimeContext.traceDir).toBe("/tmp/ralphx-logs/mcp-proxy");
    expect(env.RALPHX_AGENT_TYPE).toBe("ralphx-ideation");
    expect(env.RALPHX_AGENT_PROFILE).toBe("plan");
    expect(env.RALPHX_CONTEXT_TYPE).toBe("ideation");
    expect(env.RALPHX_CONTEXT_ID).toBe("session-123");
    expect(env.RALPHX_CONVERSATION_ID).toBe("conversation-current");
    expect(env.RALPHX_COORDINATION_MODE).toBe("rx_native_workflow");
    expect(env.RALPHX_PARENT_CONVERSATION_ID).toBe("conversation-789");
    expect(env.RALPHX_AGENT_RUN_ID).toBe("run-current");
    expect(env.RALPHX_TASK_STATE).toBe("re_executing");
    expect(env.RALPHX_PROJECT_ID).toBe("project-456");
    expect(env.RALPHX_WORKING_DIRECTORY).toBe("/tmp/workspace");
    expect(env.RALPHX_FILESYSTEM_READ_ROOTS).toBe(
      JSON.stringify(["/tmp/project", "/tmp/shared-artifacts"])
    );
    expect(env.TAURI_API_URL).toBe("http://127.0.0.1:3857");
    expect(env.RALPHX_MCP_TRACE_DIR).toBe("/tmp/ralphx-logs/mcp-proxy");
  });

  it("preserves PersonaBuilder extractor read roots in the runtime environment", () => {
    const env: NodeJS.ProcessEnv = {};

    const runtimeContext = hydrateRalphxRuntimeEnvFromCli(
      [
        "node",
        "index.js",
        "--agent-type",
        "ralphx-persona-extractor",
        "--context-type",
        "project",
        "--context-id",
        "project-persona-builder",
        "--conversation-id",
        "conversation-persona-builder",
        "--filesystem-read-root",
        "/app-data/persona_ingest/conversation-hash",
      ],
      env
    );

    const expectedReadRoots = JSON.stringify([
      "/app-data/persona_ingest/conversation-hash",
    ]);
    expect(runtimeContext.filesystemReadRoots).toBe(expectedReadRoots);
    expect(env.RALPHX_FILESYSTEM_READ_ROOTS).toBe(expectedReadRoots);
  });
});

describe("buildArtifactMutationTransportHeaders", () => {
  it("carries caller scope and live action authority for Plan mutations", () => {
    expect(
      buildArtifactMutationTransportHeaders({
        contextType: "ideation",
        contextId: "session-123",
        conversationId: "conversation-current",
        agentRunId: "run-current",
      })
    ).toEqual({
      "X-RalphX-Caller-Session-Id": "session-123",
      "x-ralphx-agent-run-id": "run-current",
      "x-ralphx-conversation-id": "conversation-current",
    });
  });

  it("does not invent partial action authority", () => {
    expect(
      buildArtifactMutationTransportHeaders({
        contextType: "project",
        conversationId: "conversation-current",
      })
    ).toBeUndefined();
  });
});
