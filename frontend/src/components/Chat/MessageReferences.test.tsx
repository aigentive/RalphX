import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";

import { MessageReferences } from "./MessageReferences";
import { useTicketingStore } from "@/stores/ticketingStore";
import { useUiStore } from "@/stores/uiStore";
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

describe("MessageReferences", () => {
  it("renders artifact reference status, version, and fallback labels", () => {
    render(
      <MessageReferences
        projectReferences={[]}
        integrationReferences={[]}
        artifactReferences={[
          {
            kind: "plan",
            artifactId: "plan-approved",
            title: "Implementation Plan",
            sessionId: "session-1",
            version: 2,
            status: "approved",
          },
          {
            kind: "artifact",
            artifactId: "accepted-artifact-1234567890",
            sessionId: "session-2",
            status: "accepted",
          },
          {
            kind: "artifact",
            artifactId: "draft-id",
            sessionId: "session-3",
            status: "needs_review",
          },
        ]}
      />,
    );

    expect(screen.getByTestId("message-reference-artifact:plan:plan-approved")).toBeTruthy();
    expect(screen.getByText("Implementation Plan")).toBeTruthy();
    expect(screen.getByText("Approved · v2")).toBeTruthy();
    expect(screen.getByText("accepted...")).toBeTruthy();
    expect(screen.getByText("Accepted")).toBeTruthy();
    expect(screen.getByText("draft-id")).toBeTruthy();
    expect(screen.getByText("Draft")).toBeTruthy();
  });

  it("renders project and integration reference chips", () => {
    render(
      <MessageReferences
        projectReferences={[
          { path: "src/main.ts", kind: "file" },
          { path: "docs/specs", kind: "directory" },
        ]}
        integrationReferences={[
          {
            provider: "atlassian",
            kind: "jira",
            id: "10001",
            key: "RX-42",
            title: "Fix composer references",
            url: "https://example.atlassian.net/browse/RX-42",
          },
          {
            provider: "atlassian",
            kind: "confluence",
            id: "20002",
            title: "Implementation Notes",
          },
        ]}
        artifactReferences={[]}
      />,
    );

    expect(screen.getByTestId("message-reference-project:src/main.ts")).toBeTruthy();
    expect(screen.getByTestId("message-reference-project:docs/specs")).toBeTruthy();
    fireEvent.click(screen.getByTestId("message-reference-integration:jira:10001"));
    expect(useTicketingStore.getState().activeProvider).toBe("jira");
    expect(useTicketingStore.getState().selectedTicketRef).toEqual({
      provider: "jira",
      id: "10001",
      key: "RX-42",
    });
    expect(useUiStore.getState().currentView).toBe("ticketing");
    expect(screen.getByText("RX-42")).toBeTruthy();
    expect(screen.getByText("Fix composer references")).toBeTruthy();
    expect(screen.getByText("Implementation Notes")).toBeTruthy();
  });

  it("opens a linear ticket chip into the ticketing view", () => {
    useTicketingStore.getState().setProvider(null);
    useTicketingStore.getState().setSelectedTicketRef(null);
    useUiStore.getState().setCurrentView("kanban");

    render(
      <MessageReferences
        projectReferences={[]}
        integrationReferences={[
          {
            provider: "linear",
            kind: "linear",
            id: "issue-uuid-1",
            key: "ENG-7",
            title: "Wire ticket chips",
          },
        ]}
        artifactReferences={[]}
      />,
    );

    fireEvent.click(
      screen.getByTestId("message-reference-integration:linear:issue-uuid-1"),
    );

    expect(useTicketingStore.getState().activeProvider).toBe("linear");
    expect(useTicketingStore.getState().selectedTicketRef).toEqual({
      provider: "linear",
      id: "issue-uuid-1",
      key: "ENG-7",
    });
    expect(useUiStore.getState().currentView).toBe("ticketing");
  });

  it("renders ticket chips as buttons (not links) so they dispatch onClick", () => {
    render(
      <MessageReferences
        projectReferences={[]}
        integrationReferences={[
          {
            provider: "linear",
            kind: "linear",
            id: "issue-uuid-2",
            key: "ENG-9",
            title: "Use a button",
            url: "https://linear.app/example/issue/ENG-9",
          },
        ]}
        artifactReferences={[]}
      />,
    );

    const chip = screen.getByTestId(
      "message-reference-integration:linear:issue-uuid-2",
    );
    // Even though a url is present, the ticket chip prefers the button branch.
    expect(chip.tagName).toBe("BUTTON");
    expect(chip.getAttribute("type")).toBe("button");
  });

  it("renders non-ticket integrations (confluence) as links without onOpenTicket", () => {
    render(
      <MessageReferences
        projectReferences={[]}
        integrationReferences={[
          {
            provider: "atlassian",
            kind: "confluence",
            id: "conf-1",
            title: "Implementation Notes",
            url: "https://example.atlassian.net/wiki/spaces/ENG/pages/conf-1",
          },
        ]}
        artifactReferences={[]}
      />,
    );

    const chip = screen.getByTestId(
      "message-reference-integration:confluence:conf-1",
    );
    expect(chip.tagName).toBe("A");
    expect(chip.tagName).not.toBe("BUTTON");
  });
});
