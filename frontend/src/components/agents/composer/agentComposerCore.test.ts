import { describe, expect, it } from "vitest";

import {
  appendInternalSkillDirectives,
  appendProjectSkillDirectives,
  detectAgentComposerTrigger,
  extractComposerArtifactTokens,
  extractComposerIntegrationTokens,
  extractComposerPathTokens,
  extractComposerSkillTokens,
  normalizeComposerArtifactReferences,
  normalizeComposerIntegrationReferences,
  normalizeComposerProjectReferences,
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

  it("detects skill triggers anywhere in a token", () => {
    const text = "Use $workspace-swe here";
    const cursor = "Use $workspace-swe".length;

    expect(detectAgentComposerTrigger(text, cursor)).toEqual({
      kind: "skill",
      query: "workspace-swe",
      rangeStart: "Use ".length,
      rangeEnd: cursor,
    });
  });

  it("detects slash commands only at the start of the current line", () => {
    expect(detectAgentComposerTrigger("/mod", 4)).toEqual({
      kind: "slash-command",
      query: "mod",
      rangeStart: 0,
      rangeEnd: 4,
    });
    expect(detectAgentComposerTrigger("Before\n/cha", "Before\n/cha".length)).toEqual({
      kind: "slash-command",
      query: "cha",
      rangeStart: "Before\n".length,
      rangeEnd: "Before\n/cha".length,
    });
    expect(detectAgentComposerTrigger("Use /chat", "Use /chat".length)).toBeNull();
    expect(detectAgentComposerTrigger("/model spark", "/model spark".length)).toBeNull();
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
    ).toEqual({
      kind: "skill",
      query: "bad",
      rangeStart: "Find @jira:RX-1".length,
      rangeEnd: "Find @jira:RX-1$bad".length,
    });
  });

  it("replaces trigger ranges and consumes one trailing space", () => {
    const text = "Open @src then continue";
    const trigger = detectAgentComposerTrigger(text, "Open @src".length);

    expect(trigger).not.toBeNull();
    expect(replaceAgentComposerTrigger(text, trigger!, "@src/main.ts ")).toEqual({
      text: "Open @src/main.ts then continue",
      cursor: "Open @src/main.ts ".length,
    });
  });

  it("extracts unique skill tokens", () => {
    expect(
      extractComposerSkillTokens("Use $review and $review plus $plan_2 $github:yeet"),
    ).toEqual(["review", "plan_2", "github:yeet"]);
  });

  it("extracts unique path tokens", () => {
    expect(extractComposerPathTokens("Read @src/main.ts and @README.md.")).toEqual([
      { path: "src/main.ts" },
      { path: "README.md" },
    ]);
  });

  it("extracts integration tokens separately from path tokens", () => {
    const text =
      "Fix @jira:rx-42 with docs @confluence:123456 and @src/main.ts using @plan:artifact-1";

    expect(extractComposerIntegrationTokens(text)).toEqual([
      { provider: "atlassian", kind: "jira", id: "RX-42", key: "RX-42" },
      { provider: "atlassian", kind: "confluence", id: "123456" },
    ]);
    expect(extractComposerPathTokens(text)).toEqual([{ path: "src/main.ts" }]);
    expect(extractComposerArtifactTokens(text)).toEqual([
      { kind: "plan", artifactId: "artifact-1" },
    ]);
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

  it("appends project skill directives with safe ids only", () => {
    expect(
      appendProjectSkillDirectives("Use this convention", [
        "skill-123",
        "skill-123",
        "../bad",
      ]),
    ).toBe("Use this convention\n\n<!-- ralphx_project_skill=skill-123 -->");
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
        } as Parameters<typeof normalizeComposerIntegrationReferences>[0][number],
        {
          provider: "atlassian",
          kind: "github",
          id: "RX-44",
        } as Parameters<typeof normalizeComposerIntegrationReferences>[0][number],
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
