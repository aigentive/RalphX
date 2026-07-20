import { describe, expect, it } from "vitest";
import {
  PersonaDraftUpdatedEventSchema,
  PersonaSchema,
} from "./persona";

describe("PersonaSchema", () => {
  it("parses camelCase frontend persona values", () => {
    expect(
      PersonaSchema.parse({
        id: "persona-1",
        slug: "focused-reviewer",
        name: "Focused Reviewer",
        description: "Reviews changes precisely.",
        content: "persona content",
        status: "active",
        version: 2,
        projectId: null,
        contentHash: "hash-2",
        sourceSessionId: null,
        createdAt: "2026-07-12T10:00:00Z",
        updatedAt: "2026-07-12T10:01:00Z",
      }),
    ).toMatchObject({ contentHash: "hash-2", projectId: null, status: "active" });
  });
});

describe("PersonaDraftUpdatedEventSchema", () => {
  it("parses a body-free draft event", () => {
    expect(
      PersonaDraftUpdatedEventSchema.parse({
        draft_id: "persona-1",
        version: 2,
        content_hash: "hash-2",
        builder_conversation_id: "conversation-1",
      }),
    ).toEqual({
      draft_id: "persona-1",
      version: 2,
      content_hash: "hash-2",
      builder_conversation_id: "conversation-1",
    });
  });
});
