import { createElement } from "react";

import { parsePersonaDocument } from "@/lib/personaContent";

import { PersonaArtifactFrontmatter } from "./PersonaArtifactFrontmatter";

/** Separates canonical Persona metadata from the Markdown body for every artifact host. */
export function preparePersonaArtifactContent(content: string) {
  const document = parsePersonaDocument(content);
  if (!document) return { content };

  return {
    content: document.body,
    preamble: createElement(PersonaArtifactFrontmatter, {
      name: document.name,
      kind: document.kind,
      description: document.description,
    }),
  };
}
