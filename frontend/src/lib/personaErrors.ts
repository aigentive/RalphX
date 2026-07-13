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
