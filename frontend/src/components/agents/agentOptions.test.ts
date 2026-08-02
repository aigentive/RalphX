import { describe, expect, it } from "vitest";

import {
  DEFAULT_AGENT_RUNTIME,
  agentEffortOptionsForModel,
  agentModelOptions,
  agentModelSupportsCodexUltra,
  defaultModelForProvider,
  normalizeRuntimeForPersistence,
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
      agentEffortOptionsForModel("codex", "gpt-5.6-sol").map((option) => option.id),
    ).toEqual(["low", "medium", "high", "xhigh", "max"]);
    expect(
      agentEffortOptionsForModel("codex", "gpt-5.6-terra").map((option) => option.id),
    ).toEqual(["low", "medium", "high", "xhigh", "max"]);
    expect(
      agentEffortOptionsForModel("codex", "gpt-5.6-luna").map((option) => option.id),
    ).toEqual(["low", "medium", "high", "xhigh", "max"]);
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

  it("uses Terra as the Codex default only when the CLI reports support", () => {
    expect(defaultModelForProvider("codex")).toBe("gpt-5.5");
    expect(defaultModelForProvider("codex", undefined, ["gpt-5.6-sol"])).toBe(
      "gpt-5.5",
    );
    expect(defaultModelForProvider("codex", undefined, ["gpt-5.6-terra"])).toBe(
      "gpt-5.6-terra",
    );
    expect(DEFAULT_AGENT_RUNTIME).toEqual({
      provider: "codex",
      modelId: "gpt-5.5",
      effort: "xhigh",
    });
    expect(
      normalizeRuntimeSelection(
        null,
        undefined,
        ["low", "medium", "high", "xhigh", "max", "ultra"],
        ["gpt-5.6-terra"],
      ),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.6-terra",
      effort: "medium",
    });
  });

  it("uses provider-specific effort descriptions", () => {
    expect(
      agentEffortOptionsForModel("codex", "gpt-5.6-sol").find(
        (option) => option.id === "max",
      )?.description,
    ).toBe("Maximum reasoning depth for the hardest problems.");
    expect(
      agentEffortOptionsForModel("codex", "gpt-5.6-sol").some(
        (option) => option.id === "ultra",
      ),
    ).toBe(false);
    expect(
      agentEffortOptionsForModel("claude", "opus").find(
        (option) => option.id === "xhigh",
      )?.description,
    ).toBe("Best setting for most coding and agentic use cases.");
    expect(
      agentEffortOptionsForModel("claude", "opus").find(
        (option) => option.id === "max",
      )?.description,
    ).toBe(
      "For intelligence-demanding tasks that justify higher usage and possible overthinking.",
    );
  });

  it("exposes Ultra separately from ordinary reasoning effort", () => {
    expect(
      agentModelSupportsCodexUltra("codex", "gpt-5.6-sol", undefined, [
        "gpt-5.6-sol",
      ]),
    ).toBe(true);
    expect(
      agentModelSupportsCodexUltra("codex", "gpt-5.6-luna", undefined, [
        "gpt-5.6-luna",
      ]),
    ).toBe(false);
    expect(agentModelSupportsCodexUltra("claude", "opus")).toBe(false);
  });

  it("capability-gates GPT-5.6 Codex models above older Codex models", () => {
    expect(agentModelOptions("codex").map((option) => option.id)).toEqual([
      "gpt-5.5",
      "gpt-5.4",
      "gpt-5.4-mini",
      "gpt-5.3-codex",
      "gpt-5.3-codex-spark",
    ]);
    expect(
      agentModelOptions("codex", undefined, [
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
        "gpt-5.5",
      ]).map((option) => option.id),
    ).toEqual([
      "gpt-5.6-sol",
      "gpt-5.6-terra",
      "gpt-5.6-luna",
      "gpt-5.5",
      "gpt-5.4",
      "gpt-5.4-mini",
      "gpt-5.3-codex",
      "gpt-5.3-codex-spark",
    ]);
    expect(
      agentModelOptions("codex", undefined, ["gpt-5.6"]).map(
        (option) => option.id,
      ),
    ).toEqual([
      "gpt-5.6-sol",
      "gpt-5.5",
      "gpt-5.4",
      "gpt-5.4-mini",
      "gpt-5.3-codex",
      "gpt-5.3-codex-spark",
    ]);
  });

  it("falls back from GPT-5.6 when Codex aliases are missing or unsupported", () => {
    expect(
      normalizeRuntimeSelection({
        provider: "codex",
        modelId: "gpt-5.6-sol",
        effort: "ultra",
      }),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.5",
      effort: "xhigh",
    });
    expect(
      normalizeRuntimeSelection(
        {
          provider: "codex",
          modelId: "gpt-5.6-sol",
          effort: "ultra",
        },
        undefined,
        null,
        ["gpt-5.5"],
      ),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.5",
      effort: "xhigh",
    });
  });

  it("keeps GPT-5.6 when Codex aliases prove availability", () => {
    expect(
      normalizeRuntimeSelection(
        {
          provider: "codex",
          modelId: "gpt-5.6",
          effort: "ultra",
        },
        undefined,
        ["low", "medium", "high", "xhigh", "max", "ultra"],
        ["gpt-5.6"],
      ),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.6-sol",
      effort: "max",
    });
    expect(
      normalizeRuntimeSelection(
        {
          provider: "codex",
          modelId: "gpt-5.6-luna",
          effort: "ultra",
        },
        undefined,
        ["low", "medium", "high", "xhigh", "max", "ultra"],
        ["gpt-5.6-luna"],
      ),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.6-luna",
      effort: "max",
    });
  });

  it("preserves known GPT-5.6 models for persistence without Codex aliases", () => {
    expect(
      normalizeRuntimeForPersistence({
        provider: "codex",
        modelId: "gpt-5.6-terra",
        effort: "ultra",
      }),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.6-terra",
      effort: "max",
    });
    expect(
      normalizeRuntimeForPersistence({
        provider: "codex",
        modelId: "gpt-5.6-luna",
        effort: "ultra",
      }),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.6-luna",
      effort: "max",
    });
    expect(
      normalizeRuntimeForPersistence({
        provider: "codex",
        modelId: "gpt-5.6",
        effort: "ultra",
      }),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.6-sol",
      effort: "max",
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
      "opus",
      "sonnet",
      "haiku",
    ]);
    expect(
      agentModelOptions("claude", undefined, ["sonnet", "opus", "haiku"]).map(
        (option) => option.id,
      ),
    ).toEqual(["opus", "sonnet", "haiku"]);
    expect(
      agentModelOptions("claude", undefined, ["sonnet", "opus", "haiku", "fable"]).map(
        (option) => option.id,
      ),
    ).toEqual(["fable", "opus", "sonnet", "haiku"]);
    expect(
      agentModelOptions("claude", undefined, [
        "sonnet",
        "opus",
        "haiku",
        "claude-sonnet-4-6",
      ]).map((option) => option.id),
    ).toEqual(["opus", "sonnet", "haiku"]);
  });

  it("progressively exposes each pinned Opus model only for its exact reported id", () => {
    const oldCliAliases = ["sonnet", "opus", "haiku", "fable"];
    const opus47Aliases = [...oldCliAliases, "  CLAUDE-OPUS-4-7  "];
    const opus48Aliases = [...opus47Aliases, "claude-opus-4-8"];
    const opus5Aliases = [...opus48Aliases, "claude-opus-5"];

    expect(agentModelOptions("claude", undefined, oldCliAliases).map((option) => option.id)).toEqual([
      "fable",
      "opus",
      "sonnet",
      "haiku",
    ]);
    expect(agentModelOptions("claude", undefined, opus47Aliases).map((option) => option.id)).toEqual([
      "fable",
      "claude-opus-4-7",
      "opus",
      "sonnet",
      "haiku",
    ]);
    expect(agentModelOptions("claude", undefined, opus48Aliases).map((option) => option.id)).toEqual([
      "fable",
      "claude-opus-4-8",
      "claude-opus-4-7",
      "opus",
      "sonnet",
      "haiku",
    ]);
    expect(agentModelOptions("claude", undefined, opus5Aliases).map((option) => option.id)).toEqual([
      "fable",
      "claude-opus-5",
      "claude-opus-4-8",
      "claude-opus-4-7",
      "opus",
      "sonnet",
      "haiku",
    ]);
    expect(agentModelOptions("claude", undefined, ["opus"]).map((option) => option.id)).toEqual([
      "opus",
      "sonnet",
      "haiku",
    ]);

    expect(
      agentModelOptions("claude", undefined, [
        "fable",
        "claude-opus-5",
        "claude-opus-4-8",
        "claude-opus-4-7",
        "opus",
        "claude-sonnet-5",
        "claude-sonnet-4-6",
        "sonnet",
        "haiku",
      ]).map((option) => option.id),
    ).toEqual([
      "fable",
      "claude-opus-5",
      "claude-opus-4-8",
      "claude-opus-4-7",
      "opus",
      "claude-sonnet-5",
      "claude-sonnet-4-6",
      "sonnet",
      "haiku",
    ]);

    for (const modelId of ["claude-opus-4-7", "claude-opus-4-8", "claude-opus-5"]) {
      expect(agentEffortOptionsForModel("claude", modelId).map((option) => option.id)).toEqual([
        "low",
        "medium",
        "high",
        "xhigh",
        "max",
      ]);
      expect(
        normalizeRuntimeSelection(
          { provider: "claude", modelId, effort: "retired-effort" },
          undefined,
          null,
          [modelId],
        ),
      ).toEqual({ provider: "claude", modelId, effort: "high" });
    }
    expect(
      normalizeRuntimeSelection(
        { provider: "claude", modelId: "claude-opus-5", effort: "xhigh" },
        undefined,
        null,
        opus48Aliases,
      ),
    ).toEqual({ provider: "claude", modelId: "sonnet", effort: "medium" });
  });

  it("progressively exposes Sonnet 5 only when Claude reports the model id", () => {
    expect(agentModelOptions("claude").map((option) => option.id)).toEqual([
      "opus",
      "sonnet",
      "haiku",
    ]);
    expect(
      agentModelOptions("claude", undefined, [
        "sonnet",
        "opus",
        "haiku",
        "fable",
        "claude-sonnet-5",
      ]).map((option) => option.id),
    ).toEqual([
      "fable",
      "opus",
      "claude-sonnet-5",
      "claude-sonnet-4-6",
      "sonnet",
      "haiku",
    ]);
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
        ["sonnet", "opus", "haiku", "claude-sonnet-4-6"],
      ),
    ).toEqual({
      provider: "claude",
      modelId: "sonnet",
      effort: "medium",
    });
  });

  it("falls back from Sonnet 5 only when provider model aliases are known unsupported", () => {
    expect(
      normalizeRuntimeSelection({
        provider: "claude",
        modelId: "claude-sonnet-5",
        effort: "xhigh",
      }),
    ).toEqual({
      provider: "claude",
      modelId: "claude-sonnet-5",
      effort: "xhigh",
    });

    expect(
      normalizeRuntimeSelection(
        {
          provider: "claude",
          modelId: "claude-sonnet-5",
          effort: "xhigh",
        },
        undefined,
        null,
        ["sonnet", "opus", "haiku", "fable"],
      ),
    ).toEqual({
      provider: "claude",
      modelId: "sonnet",
      effort: "medium",
    });
  });

  it("falls back from explicit Sonnet 4.6 while Sonnet latest is still only an alias", () => {
    expect(
      normalizeRuntimeSelection(
        {
          provider: "claude",
          modelId: "claude-sonnet-4-6",
          effort: "max",
        },
        undefined,
        null,
        ["sonnet", "opus", "haiku"],
      ),
    ).toEqual({
      provider: "claude",
      modelId: "sonnet",
      effort: "max",
    });
  });

  it("keeps explicit Sonnet 4.6 selectable once Sonnet 5 is available", () => {
    expect(
      normalizeRuntimeSelection(
        {
          provider: "claude",
          modelId: "claude-sonnet-4-6",
          effort: "max",
        },
        undefined,
        null,
        ["sonnet", "opus", "haiku", "claude-sonnet-5"],
      ),
    ).toEqual({
      provider: "claude",
      modelId: "claude-sonnet-4-6",
      effort: "max",
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
