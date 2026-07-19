import { createContext } from "react";

import type { ArtifactExcerptSource } from "./artifactSelection.types";

interface ArtifactSelectionRegistration {
  register: (element: HTMLElement, source: ArtifactExcerptSource) => () => void;
}

export const ArtifactSelectionRegistrationContext =
  createContext<ArtifactSelectionRegistration | null>(null);
