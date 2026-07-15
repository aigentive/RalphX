import type { ComposerRuntimeOption } from "./runtimeSelectorTypes";

const CURRENT_EFFORT_TONES: Record<string, string> = {
  low: "var(--status-error)",
  medium: "var(--accent-primary)",
  high: "var(--status-warning)",
  xhigh: "var(--status-success)",
  max: "var(--status-success)",
};

const POSITION_TONES = [
  "var(--status-error)",
  "var(--accent-primary)",
  "var(--status-warning)",
  "var(--status-success)",
] as const;

export function selectedOptionIndex(
  options: readonly ComposerRuntimeOption[],
  value: string,
): number {
  const index = options.findIndex((option) => option.id === value);
  return index >= 0 ? index : 0;
}

export function clampOptionIndex(index: number, optionCount: number): number {
  if (optionCount <= 0) return 0;
  return Math.min(Math.max(Math.round(index), 0), optionCount - 1);
}

export function effortTone(
  options: readonly ComposerRuntimeOption[],
  value: string,
): string {
  const currentTone = CURRENT_EFFORT_TONES[value];
  if (currentTone) return currentTone;
  if (options.length <= 1) return POSITION_TONES[0];
  const position = selectedOptionIndex(options, value) / (options.length - 1);
  const toneIndex = Math.round(position * (POSITION_TONES.length - 1));
  return POSITION_TONES[toneIndex] ?? POSITION_TONES[0];
}

export function optionIndexFromPointer(
  clientX: number,
  left: number,
  width: number,
  optionCount: number,
): number {
  if (width <= 0 || optionCount <= 1) return 0;
  const ratio = (clientX - left) / width;
  return clampOptionIndex(ratio * (optionCount - 1), optionCount);
}

export function runtimeSummary(parts: {
  providerLabel: string;
  modelLabel: string;
  effortLabel?: string;
  fastMode: boolean;
}): string {
  return [
    parts.providerLabel,
    parts.modelLabel,
    parts.effortLabel ? `${parts.effortLabel} effort` : "",
    parts.fastMode ? "Fast mode on" : "",
  ]
    .filter(Boolean)
    .join(", ");
}
