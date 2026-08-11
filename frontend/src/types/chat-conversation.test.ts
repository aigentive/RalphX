import { describe, expect, it } from "vitest";
import { ChatConversationSchema } from "./chat-conversation";

describe("ChatConversationSchema", () => {
  it("parses the persona_builder conversation mode", () => {
    expect(
      ChatConversationSchema.parse({
        id: "conversation-1",
        contextType: "project",
        contextId: "project-1",
        providerSessionId: null,
        providerHarness: null,
        agentMode: "persona_builder",
        personaId: null,
        title: null,
        messageCount: 0,
        lastMessageAt: null,
        createdAt: "2026-07-12T10:00:00Z",
        updatedAt: "2026-07-12T10:00:00Z",
      }).agentMode,
    ).toBe("persona_builder");
  });
});
