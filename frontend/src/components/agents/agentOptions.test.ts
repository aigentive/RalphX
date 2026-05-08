import { describe, expect, it } from "vitest";

import {
  DEFAULT_AGENT_RUNTIME,
  agentEffortOptionsForModel,
  normalizeRuntimeSelection,
} from "./agentOptions";

describe("agentOptions", () => {
  it("falls back to the default runtime when the remembered provider is unknown", () => {
    expect(
      normalizeRuntimeSelection({
        provider: "removed-provider",
        modelId: "retired-model",
        effort: "high",
      } as never),
    ).toEqual(DEFAULT_AGENT_RUNTIME);
  });

  it("keeps a typed custom model for a valid provider", () => {
    expect(
      normalizeRuntimeSelection({
        provider: "claude",
        modelId: "claude-opus-4-7-20260501",
        effort: "high",
      }),
    ).toEqual({
      provider: "claude",
      modelId: "claude-opus-4-7-20260501",
      effort: "high",
    });
  });

  it("keeps a valid provider/model and falls back to that model's default effort", () => {
    expect(
      normalizeRuntimeSelection({
        provider: "codex",
        modelId: "gpt-5.4-mini",
        effort: "retired-effort",
      }),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.4-mini",
      effort: "medium",
    });
  });

  it("only exposes xhigh for Codex models that support it", () => {
    expect(
      agentEffortOptionsForModel("codex", "gpt-5.5").map((option) => option.id),
    ).toEqual(["low", "medium", "high", "xhigh"]);
    expect(
      agentEffortOptionsForModel("codex", "gpt-5.4-mini").map((option) => option.id),
    ).toEqual(["low", "medium", "high"]);
  });

  it("keeps Claude max distinct from xhigh", () => {
    expect(
      agentEffortOptionsForModel("claude", "opus").map((option) => option.id),
    ).toEqual(["low", "medium", "high", "xhigh", "max"]);
    expect(
      agentEffortOptionsForModel("claude", "opus").find((option) => option.id === "max")
        ?.label,
    ).toBe("Max");
    expect(
      agentEffortOptionsForModel("claude", "sonnet").map((option) => option.id),
    ).toEqual(["low", "medium", "high", "max"]);
  });

  it("normalizes an unsupported effort to the selected model default", () => {
    expect(
      normalizeRuntimeSelection({
        provider: "codex",
        modelId: "gpt-5.4-mini",
        effort: "xhigh",
      }),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.4-mini",
      effort: "medium",
    });

    expect(
      normalizeRuntimeSelection({
        provider: "codex",
        modelId: "gpt-5.5",
        effort: "max",
      }),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.5",
      effort: "xhigh",
    });
  });
});
