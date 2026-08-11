import { describe, expect, it } from "vitest";

import {
  buildArtifactMutationTransportHeaders,
  buildRuntimeTransportHeaders,
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

describe("buildRuntimeTransportHeaders", () => {
  it("carries trusted conversation identity without requiring run authority", () => {
    expect(
      buildRuntimeTransportHeaders({ conversationId: "conversation-current" })
    ).toEqual({ "x-ralphx-conversation-id": "conversation-current" });
  });

  it("omits missing and blank conversation identity", () => {
    expect(buildRuntimeTransportHeaders({})).toBeUndefined();
    expect(buildRuntimeTransportHeaders({ conversationId: "  " })).toBeUndefined();
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

  it("hydrates filesystem enforcement from argv without reading or writing env", () => {
    const enforcedEnv: NodeJS.ProcessEnv = {};
    const enforced = hydrateRalphxRuntimeEnvFromCli(
      ["node", "index.js", "--filesystem-enforced", "1"],
      enforcedEnv
    );

    expect(enforced.filesystemEnforced).toBe(true);
    expect(enforcedEnv.RALPHX_FILESYSTEM_ENFORCED).toBeUndefined();

    const misleadingEnv: NodeJS.ProcessEnv = {
      RALPHX_FILESYSTEM_ENFORCED: "1",
    };
    const unenforced = hydrateRalphxRuntimeEnvFromCli(
      ["node", "index.js"],
      misleadingEnv
    );

    expect(unenforced.filesystemEnforced).toBe(false);
    expect(misleadingEnv.RALPHX_FILESYSTEM_ENFORCED).toBe("1");
  });

  it("denies ambient filesystem roots when enforcement has no CLI roots", () => {
    const env: NodeJS.ProcessEnv = {
      RALPHX_FILESYSTEM_READ_ROOTS: JSON.stringify(["/ambient/root"]),
    };

    const runtimeContext = hydrateRalphxRuntimeEnvFromCli(
      ["node", "index.js", "--filesystem-enforced", "1"],
      env
    );

    expect(runtimeContext.filesystemReadRoots).toBe("[]");
    expect(env.RALPHX_FILESYSTEM_READ_ROOTS).toBe("[]");
  });

  it("uses exactly the CLI filesystem roots while enforcement is enabled", () => {
    const env: NodeJS.ProcessEnv = {
      RALPHX_FILESYSTEM_READ_ROOTS: JSON.stringify(["/ambient/root"]),
    };

    const runtimeContext = hydrateRalphxRuntimeEnvFromCli(
      [
        "node",
        "index.js",
        "--filesystem-enforced",
        "1",
        "--filesystem-read-root",
        "/cli/one",
        "--filesystem-read-root=/cli/two",
      ],
      env
    );

    const expected = JSON.stringify(["/cli/one", "/cli/two"]);
    expect(runtimeContext.filesystemReadRoots).toBe(expected);
    expect(env.RALPHX_FILESYSTEM_READ_ROOTS).toBe(expected);
  });

  it("preserves legacy ambient filesystem roots when enforcement is disabled", () => {
    const ambient = JSON.stringify(["/ambient/root"]);
    const env: NodeJS.ProcessEnv = { RALPHX_FILESYSTEM_READ_ROOTS: ambient };

    const runtimeContext = hydrateRalphxRuntimeEnvFromCli(
      ["node", "index.js"],
      env
    );

    expect(runtimeContext.filesystemReadRoots).toBe(ambient);
    expect(env.RALPHX_FILESYSTEM_READ_ROOTS).toBe(ambient);
  });
});

describe("buildArtifactMutationTransportHeaders", () => {
  it("carries caller scope and live action authority for Plan mutations", () => {
    expect(
      buildArtifactMutationTransportHeaders({
        filesystemEnforced: false,
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

  it("keeps conversation identity even without action authority", () => {
    expect(
      buildArtifactMutationTransportHeaders({
        filesystemEnforced: false,
        contextType: "project",
        conversationId: "conversation-current",
      })
    ).toEqual({
      "x-ralphx-conversation-id": "conversation-current",
    });
  });
});
