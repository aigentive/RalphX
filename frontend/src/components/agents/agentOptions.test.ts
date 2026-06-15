import { describe, expect, it } from "vitest";

import {
  DEFAULT_AGENT_RUNTIME,
  agentEffortOptionsForModel,
  agentModelOptions,
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

  it("intersects model efforts with provider CLI capabilities", () => {
    const legacyClaudeEfforts = ["low", "medium", "high", "max"];

    expect(
      agentEffortOptionsForModel("claude", "opus", undefined, legacyClaudeEfforts).map(
        (option) => option.id,
      ),
    ).toEqual(["low", "medium", "high", "max"]);
    expect(
      normalizeRuntimeSelection(
        {
          provider: "claude",
          modelId: "opus",
          effort: "xhigh",
        },
        undefined,
        legacyClaudeEfforts,
      ),
    ).toEqual({
      provider: "claude",
      modelId: "opus",
      effort: "high",
    });
    expect(
      normalizeRuntimeSelection(
        {
          provider: "claude",
          modelId: "opus",
          effort: "max",
        },
        undefined,
        legacyClaudeEfforts,
      ),
    ).toEqual({
      provider: "claude",
      modelId: "opus",
      effort: "max",
    });
  });

  it("uses provider effort capabilities for typed custom models", () => {
    const providerEfforts = ["high", "max"];

    expect(
      agentEffortOptionsForModel("claude", "custom-claude-model", undefined, providerEfforts).map(
        (option) => option.id,
      ),
    ).toEqual(["high", "max"]);
    expect(
      normalizeRuntimeSelection(
        {
          provider: "claude",
          modelId: "custom-claude-model",
          effort: "retired-effort",
        },
        undefined,
        providerEfforts,
      ),
    ).toEqual({
      provider: "claude",
      modelId: "custom-claude-model",
      effort: "high",
    });
  });

  it("progressively exposes Fable only when Claude reports the alias", () => {
    expect(agentModelOptions("claude").map((option) => option.id)).toEqual([
      "sonnet",
      "opus",
      "haiku",
    ]);
    expect(
      agentModelOptions("claude", undefined, ["sonnet", "opus", "haiku"]).map(
        (option) => option.id,
      ),
    ).toEqual(["sonnet", "opus", "haiku"]);
    expect(
      agentModelOptions("claude", undefined, ["sonnet", "opus", "haiku", "fable"]).map(
        (option) => option.id,
      ),
    ).toEqual(["sonnet", "opus", "haiku", "fable"]);
  });

  it("falls back from Fable only when provider model aliases are known unsupported", () => {
    expect(
      normalizeRuntimeSelection({
        provider: "claude",
        modelId: "fable",
        effort: "xhigh",
      }),
    ).toEqual({
      provider: "claude",
      modelId: "fable",
      effort: "xhigh",
    });

    expect(
      normalizeRuntimeSelection(
        {
          provider: "claude",
          modelId: "fable",
          effort: "xhigh",
        },
        undefined,
        null,
        ["sonnet", "opus", "haiku"],
      ),
    ).toEqual({
      provider: "claude",
      modelId: "sonnet",
      effort: "medium",
    });
  });

  it("keeps custom Claude models while gating Fable aliases", () => {
    expect(
      normalizeRuntimeSelection(
        {
          provider: "claude",
          modelId: "my-fable-compatible-model",
          effort: "high",
        },
        undefined,
        null,
        ["sonnet", "opus", "haiku"],
      ),
    ).toEqual({
      provider: "claude",
      modelId: "my-fable-compatible-model",
      effort: "high",
    });
  });
});
