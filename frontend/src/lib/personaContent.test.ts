import { describe, expect, it } from "vitest";

import { parsePersonaDocument, splitPersonaBody } from "./personaContent";

describe("parsePersonaDocument", () => {
  it("separates canonical persona frontmatter from Markdown", () => {
    expect(
      parsePersonaDocument(
        "---\nname: effect-engineer\nkind: persona\ndescription: 'Reliable: even under failure'\n---\n\n# Effect Engineer\n\nUse Effect.",
      ),
    ).toEqual({
      name: "effect-engineer",
      kind: "persona",
      description: "Reliable: even under failure",
      body: "# Effect Engineer\n\nUse Effect.",
    });
  });

  it("does not reinterpret ordinary Markdown as frontmatter", () => {
    expect(parsePersonaDocument("name: prose\n\nUse direct language.")).toBeNull();
  });

  it("decodes canonical quoted descriptions", () => {
    expect(
      parsePersonaDocument(
        '---\nname: reviewer\nkind: persona\ndescription: "Calm \\"reviewer\\""\n---\nBody',
      )?.description,
    ).toBe('Calm "reviewer"');
  });

  it("rejects incomplete frontmatter instead of hiding Markdown", () => {
    expect(parsePersonaDocument("---\nname: reviewer\n---\nBody")).toBeNull();
  });

  it("keeps the editor's tolerant partial-frontmatter stripping contract", () => {
    expect(splitPersonaBody("---\nname: reviewer\n---\n\nReview carefully.")).toBe(
      "Review carefully.",
    );
  });
});
