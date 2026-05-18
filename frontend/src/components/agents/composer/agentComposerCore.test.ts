import { describe, expect, it } from "vitest";

import {
  appendInternalSkillDirectives,
  detectAgentComposerTrigger,
  extractComposerPathTokens,
  extractComposerSkillTokens,
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
});
