import { describe, expect, it } from "vitest";

import {
  parseComposerReferencesFromMetadata,
  serializeComposerReferencesMetadata,
} from "./MessageReferences.parse";

describe("parseComposerReferencesFromMetadata", () => {
  it("reads persisted composer reference metadata", () => {
    const parsed = parseComposerReferencesFromMetadata({
      composer_project_references: [{ path: "src/main.ts", kind: "file" }],
      composer_integration_references: [
        {
          provider: "atlassian",
          kind: "jira",
          id: "RX-42",
          key: "RX-42",
          title: "Fix composer references",
          url: "https://example.atlassian.net/browse/RX-42",
        },
      ],
      composer_artifact_references: [
        {
          kind: "plan",
          artifact_id: "artifact-1",
          title: "Implementation Plan",
          session_id: "session-1",
          version: 2,
          status: "approved",
        },
      ],
    });

    expect(parsed).toEqual({
      projectReferences: [{ path: "src/main.ts", kind: "file" }],
      integrationReferences: [
        {
          provider: "atlassian",
          kind: "jira",
          id: "RX-42",
          key: "RX-42",
          title: "Fix composer references",
          url: "https://example.atlassian.net/browse/RX-42",
        },
      ],
      artifactReferences: [
        {
          kind: "plan",
          artifactId: "artifact-1",
          title: "Implementation Plan",
          sessionId: "session-1",
          version: 2,
          status: "approved",
        },
      ],
    });
  });

  it("serializes selected composer references for optimistic messages", () => {
    const metadata = serializeComposerReferencesMetadata({
      projectReferences: [{ path: "src/main.ts", kind: "file" }],
      integrationReferences: [
        {
          provider: "atlassian",
          kind: "confluence",
          id: "123",
          title: "Implementation Notes",
          url: "https://example.atlassian.net/wiki/spaces/ENG/pages/123",
        },
      ],
      artifactReferences: [
        {
          kind: "plan",
          artifactId: "artifact-2",
          title: "Approved Plan",
          sessionId: "session-2",
          version: 3,
          status: "approved",
        },
      ],
    });

    expect(metadata).toBeTruthy();
    expect(parseComposerReferencesFromMetadata(JSON.parse(metadata ?? "{}"))).toEqual({
      projectReferences: [{ path: "src/main.ts", kind: "file" }],
      integrationReferences: [
        {
          provider: "atlassian",
          kind: "confluence",
          id: "123",
          title: "Implementation Notes",
          url: "https://example.atlassian.net/wiki/spaces/ENG/pages/123",
        },
      ],
      artifactReferences: [
        {
          kind: "plan",
          artifactId: "artifact-2",
          title: "Approved Plan",
          sessionId: "session-2",
          version: 3,
          status: "approved",
        },
      ],
    });
  });

  it("does not serialize empty or invalid references", () => {
    expect(
      serializeComposerReferencesMetadata({
        projectReferences: [{ path: "   ", kind: "file" }],
        integrationReferences: [
          {
            provider: "atlassian",
            kind: "jira",
            id: "",
          },
        ],
      }),
    ).toBeNull();
  });
});
