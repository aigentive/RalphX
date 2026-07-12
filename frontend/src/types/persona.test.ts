import { describe, expect, it } from "vitest";
import {
  PersonaDraftUpdatedEventSchema,
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
        contentHash: "hash-2",
        sourceSessionId: null,
        createdAt: "2026-07-12T10:00:00Z",
        updatedAt: "2026-07-12T10:01:00Z",
      }),
    ).toMatchObject({ contentHash: "hash-2", status: "active" });
  });
});

describe("persona support schemas", () => {
  it("parses the ingest manifest and body-free draft event", () => {
    expect(
      PersonaIngestManifestSchema.parse({
        copied: [{ name: "notes.md" }],
        skipped: [{ name: "image.png", reason: "unsupported type" }],
        rejected: [{ name: "link", reason: "symlink" }],
      }),
    ).toHaveProperty("copied.0.name", "notes.md");
    expect(
      PersonaDraftUpdatedEventSchema.parse({
        draft_id: "persona-1",
        version: 2,
        content_hash: "hash-2",
      }),
    ).toEqual({ draft_id: "persona-1", version: 2, content_hash: "hash-2" });
  });
});
