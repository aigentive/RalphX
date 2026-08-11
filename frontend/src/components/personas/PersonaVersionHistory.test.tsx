import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PersonaVersionHistory } from "./PersonaVersionHistory";

const history = [
  {
    id: "artifact-current",
    version: 2,
    name: "Support Voice",
    created_at: "2026-07-17T10:00:00Z",
    created_by: "user",
    metadata: { persona_version: 2, created_by: "user" },
  },
  {
    id: "artifact-old",
    version: 1,
    name: "Support Voice",
    created_at: "2026-07-17T09:00:00Z",
    created_by: "agent",
    metadata: { persona_version: 1, created_by: "agent" },
  },
];

const currentDocument = `---
name: support-voice
kind: persona
description: Calm customer support.
---
# Support Voice

Current guidance.`;

const historicalDocument = `---
name: support-voice
kind: persona
description: Original support guidance.
---
# Support Voice

Original agent draft.`;

function renderHistory() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  }
  return render(
    <PersonaVersionHistory artifactId="artifact-current" />,
    { wrapper: Wrapper },
  );
}

describe("PersonaVersionHistory", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_artifact") {
        return {
          id: "artifact-current",
          name: "Support Voice",
          artifact_type: "persona",
          content_type: "inline",
          content: currentDocument,
          created_at: "2026-07-17T10:00:00Z",
          created_by: "user",
          version: 2,
          bucket_id: "persona-library",
          task_id: null,
          process_id: null,
          derived_from: [],
        };
      }
      if (command === "get_artifact_version_history") return history;
      if (command === "get_artifact_at_version") {
        return {
          id: "artifact-current",
          name: "Support Voice",
          artifact_type: "persona",
          content_type: "inline",
          content: historicalDocument,
          created_at: "2026-07-17T09:00:00Z",
          created_by: "agent",
          version: 1,
          bucket_id: "persona-library",
          task_id: null,
          process_id: null,
          derived_from: [],
        };
      }
      throw new Error(`Unexpected command: ${command}`);
    });
  });

  it("renders current Persona metadata and Markdown through the shared artifact surface", async () => {
    renderHistory();

    expect(await screen.findByText("Calm customer support.")).toBeInTheDocument();
    expect(screen.getByTestId("persona-frontmatter")).toBeInTheDocument();
    expect(screen.getByText("Current guidance.")).toBeInTheDocument();
    expect(screen.queryByText(/name: support-voice/)).not.toBeInTheDocument();
    expect(screen.getByTitle("View version history")).toBeInTheDocument();
  });

  it("uses the shared version picker and structured rendering for historical content", async () => {
    const user = userEvent.setup();
    renderHistory();

    await user.click(await screen.findByTitle("View version history"));
    await user.click(screen.getByText(/^v1/));

    expect(await screen.findByText("Original support guidance.")).toBeInTheDocument();
    expect(screen.getByText("Original agent draft.")).toBeInTheDocument();
    expect(screen.getByText("Viewing version 1 of 2")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Back to latest" })).toBeInTheDocument();
  });

  it("shows an explicit failure when the current artifact cannot be loaded", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_artifact") throw new Error("offline");
      throw new Error(`Unexpected command: ${command}`);
    });

    renderHistory();

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Persona artifact unavailable",
    );
  });
});
