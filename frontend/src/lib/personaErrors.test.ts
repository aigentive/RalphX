import { describe, expect, it } from "vitest";
import {
  PERSONA_FEATURE_DISABLED_PREFIX,
  PERSONA_UNAVAILABLE_PREFIX,
  isPersonaFeatureDisabledError,
  isPersonaUnavailableError,
} from "./personaErrors";

describe("persona error helpers", () => {
  it("matches only unavailable-error prefixes", () => {
    expect(isPersonaUnavailableError(`${PERSONA_UNAVAILABLE_PREFIX} persona missing]`)).toBe(true);
    expect(isPersonaUnavailableError(`error: ${PERSONA_UNAVAILABLE_PREFIX} persona missing]`)).toBe(false);
  });

  it("matches only feature-disabled-error prefixes", () => {
    expect(isPersonaFeatureDisabledError(`${PERSONA_FEATURE_DISABLED_PREFIX} agent personas feature is disabled]`)).toBe(true);
    expect(isPersonaFeatureDisabledError(`error: ${PERSONA_FEATURE_DISABLED_PREFIX} agent personas feature is disabled]`)).toBe(false);
  });
});
