import { describe, expect, it } from "vitest";
import {
  IngestPersonaContextInputSchema,
  PersonaDraftUpdatedEventSchema,
  PersonaBuilderIngestStatusSchema,
  PersonaIngestEntrySchema,
  PersonaIngestManifestSchema,
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

describe("persona support schemas", () => {
  it("parses the backend ingest manifest shape and body-free draft event", () => {
    expect(
      PersonaIngestManifestSchema.parse({
        copied: [{ path: "notes.md" }],
        skipped: [{ path: "image.png", reason: "unsupported type" }],
        rejected: [{ path: "link", reason: "symlink" }],
      }),
    ).toHaveProperty("copied.0.path", "notes.md");
    expect(
      PersonaDraftUpdatedEventSchema.parse({
        draft_id: "persona-1",
        version: 2,
        content_hash: "hash-2",
      }),
    ).toEqual({ draft_id: "persona-1", version: 2, content_hash: "hash-2" });
  });

  it("parses the PersonaBuilder ingest liveness response", () => {
    expect(PersonaBuilderIngestStatusSchema.parse({ live: true })).toEqual({ live: true });
    expect(PersonaBuilderIngestStatusSchema.safeParse({ live: "yes" }).success).toBe(false);
  });

  it("parses a persona ingest entry reason", () => {
    expect(
      PersonaIngestEntrySchema.parse({
        path: "image.png",
        reason: "unsupported type",
      }),
    ).toEqual({ path: "image.png", reason: "unsupported type" });
  });

  it("round-trips the batched persona ingest input", () => {
    expect(
      IngestPersonaContextInputSchema.parse({
        conversationId: "conversation-1",
        pickedPaths: ["/tmp/one.md", "/tmp/context"],
      }),
    ).toEqual({
      conversationId: "conversation-1",
      pickedPaths: ["/tmp/one.md", "/tmp/context"],
    });
    expect(
      IngestPersonaContextInputSchema.safeParse({
        conversationId: "conversation-1",
        pickedPaths: [],
      }).success,
    ).toBe(false);
  });

  it("rejects the legacy persona ingest entry name field", () => {
    expect(PersonaIngestEntrySchema.safeParse({ name: "notes.md" }).success).toBe(false);
  });
});
