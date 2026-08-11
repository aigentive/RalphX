import type { ComposerRuntimeOption } from "./runtimeSelectorTypes";

export function buildCapabilityOptions({
  teamEnabled,
  workflowsEnabled,
  codexUltraAvailable,
}: {
  teamEnabled: boolean | undefined;
  workflowsEnabled: boolean | undefined;
  codexUltraAvailable: boolean;
}): ComposerRuntimeOption[] {
  const options: ComposerRuntimeOption[] = [
    {
      id: "solo",
      label: "Defaults",
      description: "Use the selected provider without extra orchestration.",
    },
  ];

  if (teamEnabled) {
    options.push({
      id: "rx_native_team",
      label: "Team",
      description:
        "Let this agent delegate to RalphX teammates when it helps; it may also work alone.",
    });
  }

  if (workflowsEnabled) {
    options.push({
      id: "rx_native_workflow",
      label: "Workflow",
      description: "Generate and run a durable reviewed orchestration script.",
    });
  }

  if (codexUltraAvailable) {
    options.push({
      id: "codex_native_ultra",
      label: "Ultra",
      description: "Activate Codex provider-native subagents and maximum reasoning.",
    });
  }

  return options;
}
