/**
 * Copy for every skip reason the backend can emit. Keep in sync with the reason
 * strings in `src-tauri/src/infrastructure/agents/claude/mod.rs`.
 */
const PERSONA_SKIPPED_REASON_COPY: Record<string, string> = {
  native_agent_flag: "Native agent mode does not support personas",
  agent_prompt_not_found_native_agent:
    "The agent's prompt could not be found, so the persona was not applied",
  prompt_composition_fallback_native_agent:
    "The agent fell back to a built-in prompt, so the persona was not applied",
  codex_plugin_dir_unavailable:
    "The Codex agent configuration was unavailable, so the persona was not applied",
  codex_agent_unavailable:
    "The Codex agent identity was unavailable, so the persona was not applied",
  codex_agent_prompt_unavailable:
    "The Codex agent prompt was unavailable, so the persona was not applied",
  persona_not_injected: "The persona could not be applied to this run",
  unknown: "The persona could not be applied to this run",
};

const DEFAULT_PERSONA_SKIPPED_REASON =
  "The persona could not be applied to this run";

export function getPersonaSkippedReasonCopy(
  skippedReason: string | null | undefined,
): string {
  return (
    (skippedReason && PERSONA_SKIPPED_REASON_COPY[skippedReason]) ??
    DEFAULT_PERSONA_SKIPPED_REASON
  );
}
