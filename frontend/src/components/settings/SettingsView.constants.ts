/**
 * Constants for SettingsView components
 *
 * Extracted to satisfy react-refresh/only-export-components lint rule.
 */

import type { Model } from "@/types/agent-profile";

export const MODEL_OPTIONS: { value: Model; label: string; description: string }[] = [
  {
    value: "fable",
    label: "Claude Fable 5",
    description: "Most capable, requires Claude Code 2.1.170+",
  },
  {
    value: "opus",
    label: "Claude Opus 4.5",
    description: "Most capable, best for complex tasks",
  },
  {
    value: "sonnet",
    label: "Claude Sonnet",
    description: "Best balance of speed and quality",
  },
  {
    value: "haiku",
    label: "Claude Haiku 4.5",
    description: "Fastest, most cost-effective",
  },
];
