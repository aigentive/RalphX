import { describe, expect, it } from "vitest";

import {
  migrateAgentSessionStore,
  useAgentSessionStore,
} from "./agentSessionStore";

describe("agentSessionStore", () => {
  it("defaults the Agents sidebar to all projects", () => {
    expect(useAgentSessionStore.getInitialState().showAllProjects).toBe(true);
  });

  it("migrates older persisted sidebar filter state to all projects", () => {
    expect(
      migrateAgentSessionStore(
        {
          showAllProjects: false,
          projectSort: "latest",
        },
        0,
      ),
    ).toMatchObject({
      showAllProjects: true,
    });
  });

  it("preserves current persisted sidebar filter state", () => {
    expect(
      migrateAgentSessionStore(
        {
          showAllProjects: false,
          projectSort: "latest",
        },
        1,
      ),
    ).toMatchObject({
      showAllProjects: false,
    });
  });

  it("migrates remembered runtimes to include model-specific effort", () => {
    expect(
      migrateAgentSessionStore(
        {
          runtimeByConversationId: {
            "conversation-1": {
              provider: "codex",
              modelId: "gpt-5.4-mini",
            },
          },
          lastRuntimeByProjectId: {
            "project-1": {
              provider: "claude",
              modelId: "opus",
            },
          },
        },
        1,
      ),
    ).toMatchObject({
      runtimeByConversationId: {
        "conversation-1": {
          provider: "codex",
          modelId: "gpt-5.4-mini",
          effort: "medium",
        },
      },
      lastRuntimeByProjectId: {
        "project-1": {
          provider: "claude",
          modelId: "opus",
          effort: "xhigh",
        },
      },
    });
  });

  it("preserves valid remembered runtime efforts during migration", () => {
    expect(
      migrateAgentSessionStore(
        {
          lastRuntimeByProjectId: {
            "project-1": {
              provider: "claude",
              modelId: "opus",
              effort: "high",
            },
          },
        },
        1,
      ),
    ).toMatchObject({
      lastRuntimeByProjectId: {
        "project-1": {
          provider: "claude",
          modelId: "opus",
          effort: "high",
        },
      },
    });
  });
});
