export const PERSONA_UNAVAILABLE_PREFIX = "[Persona unavailable:";
export const PERSONA_FEATURE_DISABLED_PREFIX = "[Personas disabled:";

export function isPersonaUnavailableError(error: unknown): boolean {
  return typeof error === "string" && error.startsWith(PERSONA_UNAVAILABLE_PREFIX);
}

export function isPersonaFeatureDisabledError(error: unknown): boolean {
  return (
    typeof error === "string" &&
    error.startsWith(PERSONA_FEATURE_DISABLED_PREFIX)
  );
}

export function formatPersonaErrorMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  if (isPersonaUnavailableError(message)) {
    return "This persona is no longer available.";
  }
  if (isPersonaFeatureDisabledError(message)) {
    return "Personas are disabled.";
  }
  return message || "Unable to save persona.";
}
