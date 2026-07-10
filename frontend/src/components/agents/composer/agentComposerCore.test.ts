import { describe, expect, it } from "vitest";

import {
  appendInternalSkillDirectives,
  detectAgentComposerTrigger,
  extractComposerArtifactTokens,
  extractComposerIntegrationTokens,
  extractComposerPathTokens,
  extractComposerSlashSkillTokens,
  extractPastedAtlassianResourceUrls,
  normalizeComposerArtifactReferences,
  normalizeComposerIntegrationReferences,
  normalizeComposerProjectReferences,
  removeResolvedAtlassianResourceUrls,
  replaceAgentComposerTrigger,
} from "./agentComposerCore";

describe("agentComposerCore", () => {
  it("detects path triggers in the current token", () => {
    const text = "Please inspect @src/comp";

    expect(detectAgentComposerTrigger(text, text.length)).toEqual({
      kind: "path",
      query: "src/comp",
      rangeStart: "Please inspect ".length,
      rangeEnd: text.length,
    });
  });

  it("detects slash skill triggers at the current token start", () => {
    const text = "Use /workspace-swe here";
    const cursor = "Use /workspace-swe".length;

    expect(detectAgentComposerTrigger(text, cursor)).toEqual({
      kind: "skill",
      query: "workspace-swe",
      rangeStart: "Use ".length,
      rangeEnd: cursor,
    });
  });

  it("does not detect dollar skill triggers", () => {
    const text = "Use $workspace-swe here";

    expect(
      detectAgentComposerTrigger(text, "Use $workspace-swe".length),
    ).toBeNull();
  });

  it("ignores slash skill triggers with nested markers", () => {
    expect(
      detectAgentComposerTrigger(
        "Use /workspace/swe",
        "Use /workspace/swe".length,
      ),
    ).toBeNull();
    expect(
      detectAgentComposerTrigger(
        "Use /workspace@swe",
        "Use /workspace@swe".length,
      ),
    ).toBeNull();
    expect(
      detectAgentComposerTrigger(
        "Use /workspace$swe",
        "Use /workspace$swe".length,
      ),
    ).toBeNull();
  });

  it("detects slash commands at line start and slash skills in message text", () => {
    expect(detectAgentComposerTrigger("/mod", 4)).toEqual({
      kind: "slash-command",
      query: "mod",
      rangeStart: 0,
      rangeEnd: 4,
    });
    expect(
      detectAgentComposerTrigger("Before\n/cha", "Before\n/cha".length),
    ).toEqual({
      kind: "slash-command",
      query: "cha",
      rangeStart: "Before\n".length,
      rangeEnd: "Before\n/cha".length,
    });
    expect(
      detectAgentComposerTrigger("Use /chat", "Use /chat".length),
    ).toEqual({
      kind: "skill",
      query: "chat",
      rangeStart: "Use ".length,
      rangeEnd: "Use /chat".length,
    });
    expect(
      detectAgentComposerTrigger("/model spark", "/model spark".length),
    ).toBeNull();
  });

  it("detects scoped Atlassian reference triggers under @", () => {
    const text = "Attach @jira:RX-42";

    expect(detectAgentComposerTrigger(text, text.length)).toEqual({
      kind: "integration",
      integrationKind: "jira",
      query: "RX-42",
      rangeStart: "Attach ".length,
      rangeEnd: text.length,
    });
  });

  it("detects scoped integration triggers inside the current token", () => {
    const text = "Attach foo@jira:RX-42";

    expect(detectAgentComposerTrigger(text, text.length)).toEqual({
      kind: "integration",
      integrationKind: "jira",
      query: "RX-42",
      rangeStart: "Attach foo".length,
      rangeEnd: text.length,
    });
  });

  it("detects scoped Linear issue triggers under @", () => {
    const text = "Attach @linear:LIN-123";

    expect(detectAgentComposerTrigger(text, text.length)).toEqual({
      kind: "integration",
      integrationKind: "linear",
      query: "LIN-123",
      rangeStart: "Attach ".length,
      rangeEnd: text.length,
    });
  });

  it("detects scoped ClickUp task triggers under @", () => {
    const text = "Attach @clickup:TASK-123";

    expect(detectAgentComposerTrigger(text, text.length)).toEqual({
      kind: "integration",
      integrationKind: "clickup",
      query: "TASK-123",
      rangeStart: "Attach ".length,
      rangeEnd: text.length,
    });
  });

  it("detects plan reference triggers under @", () => {
    const text = "Use @plan:checkout";

    expect(detectAgentComposerTrigger(text, text.length)).toEqual({
      kind: "plan",
      query: "checkout",
      rangeStart: "Use ".length,
      rangeEnd: text.length,
    });
  });

  it("keeps scoped Atlassian trigger queries active across spaces", () => {
    const text = "Find @jira:closed issue summary";

    expect(detectAgentComposerTrigger(text, text.length)).toEqual({
      kind: "integration",
      integrationKind: "jira",
      query: "closed issue summary",
      rangeStart: "Find ".length,
      rangeEnd: text.length,
    });
  });

  it("detects Confluence alias triggers after quoted boundaries", () => {
    const text = 'Attach "@conf:release checklist';

    expect(detectAgentComposerTrigger(text, text.length)).toEqual({
      kind: "integration",
      integrationKind: "confluence",
      query: "release checklist",
      rangeStart: 'Attach "'.length,
      rangeEnd: text.length,
    });
  });

  it("falls back to nested markers after malformed Atlassian trigger queries", () => {
    expect(
      detectAgentComposerTrigger(
        "Find @jira:RX-1@bad",
        "Find @jira:RX-1@bad".length,
      ),
    ).toEqual({
      kind: "path",
      query: "bad",
      rangeStart: "Find @jira:RX-1".length,
      rangeEnd: "Find @jira:RX-1@bad".length,
    });
    expect(
      detectAgentComposerTrigger(
        "Find @jira:RX-1$bad",
        "Find @jira:RX-1$bad".length,
      ),
    ).toBeNull();
  });

  it("replaces trigger ranges and consumes one trailing space", () => {
    const text = "Open @src then continue";
    const trigger = detectAgentComposerTrigger(text, "Open @src".length);

    expect(trigger).not.toBeNull();
    expect(
      replaceAgentComposerTrigger(text, trigger!, "@src/main.ts "),
    ).toEqual({
      text: "Open @src/main.ts then continue",
      cursor: "Open @src/main.ts ".length,
    });
  });

  it("extracts unique slash skill tokens and ignores dollar tokens", () => {
    expect(
      extractComposerSlashSkillTokens(
        "Use /review and /review plus /workspace-swe $github:yeet /github:yeet",
      ),
    ).toEqual(["review", "workspace-swe", "github:yeet"]);
  });

  it("extracts unique path tokens", () => {
    expect(
      extractComposerPathTokens("Read @src/main.ts and @README.md."),
    ).toEqual([{ path: "src/main.ts" }, { path: "README.md" }]);
  });

  it("extracts integration tokens separately from path tokens", () => {
    const text =
      "Fix @jira:rx-42 with @linear:lin-123 and @clickup:task-123 and docs @confluence:123456 and @src/main.ts using @plan:artifact-1";

    expect(extractComposerIntegrationTokens(text)).toEqual([
      { provider: "atlassian", kind: "jira", id: "RX-42", key: "RX-42" },
      { provider: "linear", kind: "linear", id: "LIN-123", key: "LIN-123" },
      { provider: "clickup", kind: "clickup", id: "TASK-123", key: "TASK-123" },
      { provider: "atlassian", kind: "confluence", id: "123456" },
    ]);
    expect(extractComposerPathTokens(text)).toEqual([{ path: "src/main.ts" }]);
    expect(extractComposerArtifactTokens(text)).toEqual([
      { kind: "plan", artifactId: "artifact-1" },
    ]);
  });

  it("extracts plausible pasted Atlassian resource URLs", () => {
    expect(
      extractPastedAtlassianResourceUrls(
        "See https://example.atlassian.net/browse/rx-42, docs https://example.atlassian.net/wiki/spaces/OPS/pages/123456/Deploy and https://example.com/browse/RX-99 plus https://example.atlassian.net/admin and https://%",
      ),
    ).toEqual([
      "https://example.atlassian.net/browse/rx-42",
      "https://example.atlassian.net/wiki/spaces/OPS/pages/123456/Deploy",
      "https://example.com/browse/RX-99",
    ]);
  });

  it("removes only backend-resolved pasted Atlassian URLs", () => {
    expect(
      removeResolvedAtlassianResourceUrls(
        "See https://example.atlassian.net/browse/RX-42 and https://other.atlassian.net/browse/RX-99",
        ["", "https://example.atlassian.net/browse/RX-42"],
      ),
    ).toBe("See and https://other.atlassian.net/browse/RX-99");
  });

  it("appends internal skill directives with safe lowercase names only", () => {
    expect(
      appendInternalSkillDirectives("Build this", [
        "workspace-swe",
        "workspace-swe",
        "../bad",
      ]),
    ).toBe("Build this\n\n<!-- ralphx_internal_skill=workspace-swe -->");
  });

  it("normalizes project references without encoding them into prompt text", () => {
    expect(
      normalizeComposerProjectReferences([
        { path: "src/main.ts", kind: "file" },
        { path: "docs/My File.md", kind: "file" },
        { path: "src/main.ts", kind: "file" },
      ]),
    ).toEqual([
      { path: "src/main.ts", kind: "file" },
      { path: "docs/My File.md", kind: "file" },
    ]);
  });

  it("normalizes integration references without duplicate Jira keys", () => {
    expect(
      normalizeComposerIntegrationReferences([
        { provider: "atlassian", kind: "jira", id: "RX-42", key: "RX-42" },
        { provider: "atlassian", kind: "jira", id: "RX-42", key: "RX-42" },
        { provider: "atlassian", kind: "confluence", id: "123", title: "Spec" },
      ]),
    ).toEqual([
      { provider: "atlassian", kind: "jira", id: "RX-42", key: "RX-42" },
      { provider: "atlassian", kind: "confluence", id: "123", title: "Spec" },
    ]);
  });

  it("normalizes artifact references without prompt text expansion", () => {
    expect(
      normalizeComposerArtifactReferences([
        {
          kind: "plan",
          artifactId: " artifact-1 ",
          title: " Plan ",
          sessionId: " session-1 ",
          version: 2,
          status: "approved",
        },
        { kind: "plan", artifactId: "artifact-1", version: 2 },
      ]),
    ).toEqual([
      {
        kind: "plan",
        artifactId: "artifact-1",
        title: "Plan",
        sessionId: "session-1",
        version: 2,
        status: "approved",
      },
    ]);
  });

  it("normalizes integration references by trimming metadata and dropping invalid entries", () => {
    expect(
      normalizeComposerIntegrationReferences([
        {
          provider: "atlassian",
          kind: "jira",
          id: " RX-42 ",
          key: " RX-42 ",
          title: " Fix composer ",
          url: " https://example.atlassian.net/browse/RX-42 ",
        },
        {
          provider: "external",
          kind: "jira",
          id: "RX-43",
        } as Parameters<
          typeof normalizeComposerIntegrationReferences
        >[0][number],
        {
          provider: "atlassian",
          kind: "github",
          id: "RX-44",
        } as Parameters<
          typeof normalizeComposerIntegrationReferences
        >[0][number],
        { provider: "atlassian", kind: "confluence", id: "bad\0id" },
      ]),
    ).toEqual([
      {
        provider: "atlassian",
        kind: "jira",
        id: "RX-42",
        key: "RX-42",
        title: "Fix composer",
        url: "https://example.atlassian.net/browse/RX-42",
      },
    ]);
  });
});
