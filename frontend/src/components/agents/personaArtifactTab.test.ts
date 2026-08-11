import { describe, expect, it } from "vitest";

import { isPersonaArtifactConversation } from "./personaArtifactTab";

describe("isPersonaArtifactConversation", () => {
  it("enables the Persona tab only for persona_builder mode", () => {
    expect(isPersonaArtifactConversation({ agentMode: "persona_builder" })).toBe(true);
    expect(isPersonaArtifactConversation({ agentMode: "automation" })).toBe(false);
    expect(isPersonaArtifactConversation({ agentMode: "chat" })).toBe(false);
    expect(isPersonaArtifactConversation(null)).toBe(false);
  });
});
