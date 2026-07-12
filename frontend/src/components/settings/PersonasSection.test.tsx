import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { TooltipProvider } from "@/components/ui/tooltip";
import { describe, expect, it, vi } from "vitest";

import { PersonasSection } from "./PersonasSection";

const toastError = vi.hoisted(() => vi.fn());

vi.mock("sonner", () => ({
  toast: { error: toastError },
}));

type RawPersona = {
  id: string;
  slug: string;
  name: string;
  description: string;
  content: string;
  status: "draft" | "active" | "archived";
  version: number;
  content_hash: string;
  source_session_id: null;
  created_at: string;
  updated_at: string;
};

const activePersona: RawPersona = {
  id: "persona-active",
  slug: "reviewer-voice",
  name: "Reviewer Voice",
  description: "A careful reviewer.",
  content: "---\nname: reviewer-voice\n---\nReview carefully.",
  status: "active",
  version: 3,
  content_hash: "active-hash",
  source_session_id: null,
  created_at: "2026-07-10T10:00:00Z",
  updated_at: "2026-07-12T08:00:00Z",
};

const draftPersona: RawPersona = {
  ...activePersona,
  id: "persona-draft",
  slug: "terse-arch",
  name: "Terse Architect",
  status: "draft",
  version: 1,
  content_hash: "draft-hash",
};

const archivedPersona: RawPersona = {
  ...activePersona,
  id: "persona-archived",
  slug: "old-voice",
  name: "Old Voice",
  status: "archived",
};

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });
}

function renderSection() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <TooltipProvider delayDuration={0}>
        <PersonasSection />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

function mockPersonaCommands(personas: RawPersona[]) {
  const store = personas.map((persona) => ({ ...persona }));
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    const input = (args as { input?: Record<string, string> } | undefined)?.input;
    if (command === "list_personas") return store;
    if (command === "create_persona_draft") {
      const created: RawPersona = {
        ...draftPersona,
        id: "persona-new",
        slug: input?.slug ?? "new-persona",
        name: "New Persona",
        content: input?.content ?? "",
      };
      store.push(created);
      return created;
    }
    if (command === "update_persona") {
      return { ...activePersona, content: input?.content ?? activePersona.content };
    }
    if (command === "approve_persona") {
      const persona = store.find((item) => item.id === input?.id);
      if (!persona) throw new Error("persona missing");
      persona.status = "active";
      return persona;
    }
    if (command === "archive_persona") {
      const persona = store.find((item) => item.id === input?.id);
      if (!persona) throw new Error("persona missing");
      persona.status = "archived";
      return persona;
    }
    if (command === "delete_persona_draft") {
      const index = store.findIndex((item) => item.id === input?.id);
      if (index >= 0) store.splice(index, 1);
      return undefined;
    }
    throw new Error(`Unexpected command: ${command}`);
  });
}

describe("PersonasSection", () => {
  it("filters archived personas from the list and renders the v1 limits copy", async () => {
    mockPersonaCommands([activePersona, draftPersona, archivedPersona]);
    renderSection();

    expect(await screen.findByText("Reviewer Voice")).toBeInTheDocument();
    expect(screen.getByText("Terse Architect")).toBeInTheDocument();
    expect(screen.queryByText("Old Voice")).not.toBeInTheDocument();
    expect(screen.getByText(/applies to this conversation only/i)).toBeInTheDocument();
    expect(screen.getByText(/delegated, subagent, or pipeline work/i)).toBeInTheDocument();
  });

  it("keeps the builder entry hidden by default", async () => {
    mockPersonaCommands([activePersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    expect(screen.queryByRole("button", { name: "Build with agent" })).not.toBeInTheDocument();
  });

  it("creates a draft, returns to the list, then activates it", async () => {
    const user = userEvent.setup();
    const personas = [activePersona];
    mockPersonaCommands(personas);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "New persona" }));
    await user.type(screen.getByLabelText("Slug"), "new-persona");
    await user.type(screen.getByLabelText("Persona content"), "---\nname: new-persona\n---");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await screen.findByText("New Persona");
    expect(invoke).toHaveBeenCalledWith("create_persona_draft", {
      input: { slug: "new-persona", content: "---\nname: new-persona\n---" },
    });

    await user.click(screen.getByRole("button", { name: "Activate New Persona" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("approve_persona", {
        input: { id: "persona-new" },
      }),
    );
  });

  it("shows a save validation error inline and keeps the active edit form populated", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_personas") return [activePersona];
      if (command === "update_persona") {
        throw new Error("body contains blocked structural tag");
      }
      throw new Error(`Unexpected command: ${command} ${String(args)}`);
    });
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "Edit Reviewer Voice" }));
    const content = screen.getByLabelText("Persona content");
    fireEvent.change(content, { target: { value: "<blocked-tag>" } });
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText(/Save failed: body contains blocked structural tag/)).toBeInTheDocument();
    expect(screen.getByLabelText("Persona content")).toHaveValue("<blocked-tag>");
    expect(toastError).not.toHaveBeenCalled();
  });

  it("updates an active persona with the hook input contract", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "Edit Reviewer Voice" }));
    fireEvent.change(screen.getByLabelText("Persona content"), {
      target: { value: "Updated body" },
    });
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_persona", {
        input: { id: "persona-active", content: "Updated body" },
      }),
    );
  });

  it("opens drafts read-only with the builder-only explanation", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([draftPersona]);
    renderSection();

    await screen.findByText("Terse Architect");
    await user.click(screen.getByRole("button", { name: "Edit Terse Architect" }));

    expect(screen.getByText("Drafts are iterated with the builder agent")).toBeInTheDocument();
    expect(screen.getByLabelText("Persona content")).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
  });

  it("archives active personas and hard-deletes drafts through confirmation", async () => {
    const user = userEvent.setup();
    const personas = [activePersona, draftPersona];
    mockPersonaCommands(personas);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "Archive Reviewer Voice" }));
    expect(await screen.findByText(/archive clears conversation bindings/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Archive persona" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("archive_persona", {
        input: { id: "persona-active" },
      }),
    );

    await user.click(screen.getByRole("button", { name: "Delete Terse Architect" }));
    await user.click(screen.getByRole("button", { name: "Delete draft" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("delete_persona_draft", {
        input: { id: "persona-draft" },
      }),
    );
  });

  it("gives destructive icon-only actions labels and app tooltips", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona, draftPersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    const archive = screen.getByRole("button", { name: "Archive Reviewer Voice" });
    const removeDraft = screen.getByRole("button", { name: "Delete Terse Architect" });
    expect(archive).toHaveAttribute("aria-label", "Archive Reviewer Voice");
    expect(removeDraft).toHaveAttribute("aria-label", "Delete Terse Architect");

    await user.hover(archive);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Archive Reviewer Voice");
  });
});
