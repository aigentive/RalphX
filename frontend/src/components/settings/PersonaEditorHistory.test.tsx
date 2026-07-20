import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import type { Persona } from "@/types/persona";

import { PersonaEditor } from "./PersonaEditor";

const persona: Persona = {
  id: "persona-1",
  artifactId: "artifact-3",
  slug: "support-voice",
  name: "Support Voice",
  description: "Calm customer support.",
  content: "## Voice\n\nEmpathetic, direct.",
  status: "active",
  version: 3,
  projectId: null,
  contentHash: "hash-3",
  sourceSessionId: null,
  sourcePersonaId: null,
  sourceContentHash: null,
  createdAt: "2026-07-17T08:00:00Z",
  updatedAt: "2026-07-17T10:00:00Z",
};

const rawHistory = [
  {
    id: "artifact-3",
    version: 3,
    name: "Support Voice",
    created_at: "2026-07-17T10:00:00Z",
    created_by: "user",
    metadata: { persona_version: 3, created_by: "user" },
  },
  {
    id: "artifact-2",
    version: 2,
    name: "Support Voice",
    created_at: "2026-07-17T09:00:00Z",
    created_by: "agent",
    metadata: { persona_version: 2, created_by: "agent" },
  },
];

const currentDocument = `---
name: support-voice
kind: persona
description: Calm customer support.
---
# Support Voice

Empathetic, direct.`;

const historicalDocument = `---
name: support-voice
kind: persona
description: Original support guidance.
---
# Support Voice

Original agent draft.`;

function renderEditor(value: Persona) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={0}>{children}</TooltipProvider>
      </QueryClientProvider>
    );
  }
  return render(
    <PersonaEditor
      editor={{ kind: "edit", persona: value }}
      projects={[]}
      projectNames={{}}
      onBack={vi.fn()}
    />,
    { wrapper: Wrapper },
  );
}

describe("PersonaEditor version history", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_artifact") {
        return {
          id: "artifact-3",
          name: "Support Voice",
          artifact_type: "persona",
          content_type: "inline",
          content: currentDocument,
          created_at: "2026-07-17T10:00:00Z",
          created_by: "user",
          version: 3,
          bucket_id: "persona-library",
          task_id: null,
          process_id: null,
          derived_from: [],
        };
      }
      if (command === "get_artifact_version_history") return rawHistory;
      if (command === "get_artifact_at_version") {
        return {
          id: "artifact-2",
          name: "Support Voice",
          artifact_type: "persona",
          content_type: "inline",
          content: historicalDocument,
          created_at: "2026-07-17T09:00:00Z",
          created_by: "agent",
          version: 2,
          bucket_id: "persona-library",
          task_id: null,
          process_id: null,
          derived_from: [],
        };
      }
      throw new Error(`Unexpected command: ${command}`);
    });
  });

  it.each(["draft", "active"] as const)(
    "shows Version history for an artifact-backed %s persona",
    (status) => {
      renderEditor({ ...persona, status });
      expect(screen.getByRole("button", { name: "Version history" })).toBeInTheDocument();
    },
  );

  it("hides Version history when the persona has no artifact id", () => {
    renderEditor({ ...persona, artifactId: null });
    expect(screen.queryByRole("button", { name: "Version history" })).not.toBeInTheDocument();
  });

  it("opens attributed history rows and keeps a historical version read-only", async () => {
    const user = userEvent.setup();
    renderEditor(persona);

    await user.click(screen.getByRole("button", { name: "Version history" }));
    expect(screen.getByRole("dialog", { name: "Version history" })).toBeInTheDocument();
    expect(await screen.findByText("Calm customer support.")).toBeInTheDocument();
    await user.click(screen.getByTitle("View version history"));
    await user.click(screen.getByText(/^v2/));
    expect(await screen.findByText("Original agent draft.")).toBeInTheDocument();
    expect(screen.getByText("Original support guidance.")).toBeInTheDocument();
    expect(screen.getByText("Viewing version 2 of 3")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Back to latest" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Instructions" })).not.toBeInTheDocument();
  });
});
