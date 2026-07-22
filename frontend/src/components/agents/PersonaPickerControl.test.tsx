import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TooltipProvider } from "@/components/ui/tooltip";
import { beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";

import { fetchPersonas, usePersonas } from "@/hooks/usePersonas";
import type { Persona } from "@/types/persona";
import { PersonaPickerControl } from "./PersonaPickerControl";

vi.mock("@/hooks/usePersonas", () => ({
  fetchPersonas: vi.fn(),
  personaKeys: { list: () => ["personas", "list"] as const },
  usePersonas: vi.fn(),
}));

const personas: Persona[] = [
  {
    id: "reviewer",
    slug: "reviewer-voice",
    name: "Reviewer Voice",
    description: "Careful reviews",
    content: "# Reviewer",
    status: "active",
    version: 1,
    projectId: null,
    contentHash: "hash-reviewer",
    sourceSessionId: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  },
  {
    id: "architect",
    slug: "terse-architect",
    name: "Terse Architect",
    description: "Short architecture notes",
    content: "# Architect",
    status: "active",
    version: 1,
    projectId: "project-current",
    contentHash: "hash-architect",
    sourceSessionId: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  },
  {
    id: "draft",
    slug: "not-ready",
    name: "Not ready",
    description: "Draft",
    content: "# Draft",
    status: "draft",
    version: 1,
    projectId: "project-current",
    contentHash: "hash-draft",
    sourceSessionId: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  },
];

function renderControl(overrides: Partial<React.ComponentProps<typeof PersonaPickerControl>> = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const prefetchQuery = vi.spyOn(queryClient, "prefetchQuery");
  const onOpenPersonas = vi.fn();
  const onValueChange = vi.fn();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={0}>
        <PersonaPickerControl
          currentProjectId="project-current"
          currentProjectName="RalphX"
          personaId={null}
          onValueChange={onValueChange}
          onOpenPersonas={onOpenPersonas}
          {...overrides}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
  return { ...view, onOpenPersonas, onValueChange, prefetchQuery };
}

describe("PersonaPickerControl", () => {
  beforeEach(() => {
    vi.mocked(fetchPersonas).mockResolvedValue(personas);
    vi.mocked(usePersonas).mockReturnValue({
      data: personas,
      isLoading: false,
    } as ReturnType<typeof usePersonas>);
  });

  it("uses a labeled pill trigger that names the selected persona", async () => {
    renderControl({ personaId: "reviewer" });

    const trigger = screen.getByRole("button", { name: "Choose persona" });
    expect(screen.getByTestId("persona-picker-label")).toHaveTextContent(
      "Reviewer Voice",
    );
    await userEvent.setup().hover(trigger);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "Persona: Reviewer Voice",
    );
  });

  it("falls back to the generic pill label when nothing is selected", () => {
    renderControl();
    expect(screen.getByTestId("persona-picker-label")).toHaveTextContent("Persona");
  });

  it("lists active personas and No persona as the default choice", () => {
    renderControl();
    fireEvent.click(screen.getByRole("button", { name: "Choose persona" }));

    expect(screen.getByRole("menuitemradio", { name: "No persona" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.getByRole("menuitemradio", { name: /^Reviewer Voice/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: /^Terse Architect/ })).toBeInTheDocument();
    expect(screen.queryByText("Not ready")).not.toBeInTheDocument();
  });

  it("shows persona descriptions and a read-only inspect preview per row", () => {
    renderControl();
    fireEvent.click(screen.getByRole("button", { name: "Choose persona" }));

    expect(screen.getByText("Careful reviews")).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Inspect Reviewer Voice" }),
    );
    expect(
      screen.getByRole("dialog", { name: "Reviewer Voice · v1" }),
    ).toBeInTheDocument();
    expect(screen.getByTestId("persona-inspect-content")).toHaveTextContent(
      "# Reviewer",
    );
  });

  it("groups global and current-project personas and excludes other projects", () => {
    const otherProjectPersona: Persona = {
      ...personas[1],
      id: "other-project",
      name: "Other Project Voice",
      projectId: "project-other",
    };
    vi.mocked(usePersonas).mockReturnValue({
      data: [...personas, otherProjectPersona],
      isLoading: false,
    } as ReturnType<typeof usePersonas>);

    renderControl();
    fireEvent.click(screen.getByRole("button", { name: "Choose persona" }));

    expect(screen.getByRole("group", { name: "Global" })).toHaveTextContent(
      "Reviewer Voice",
    );
    expect(screen.getByRole("group", { name: "RalphX" })).toHaveTextContent(
      "Terse Architect",
    );
    expect(screen.queryByText("Other Project Voice")).not.toBeInTheDocument();
    expect(usePersonas).toHaveBeenCalledWith({
      type: "globalAndProject",
      projectId: "project-current",
    });
  });

  it("changes selection and opens persona settings from the popover", () => {
    const { onOpenPersonas, onValueChange } = renderControl();
    fireEvent.click(screen.getByRole("button", { name: "Choose persona" }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: /^Reviewer Voice/ }));
    expect(onValueChange).toHaveBeenCalledWith("reviewer");

    fireEvent.click(screen.getByRole("menuitem", { name: /Manage personas/ }));
    expect(onOpenPersonas).toHaveBeenCalledOnce();
  });

  it("prefetches on intent without blocking an immediate popover shell", async () => {
    vi.mocked(usePersonas).mockReturnValue({
      data: undefined,
      isLoading: true,
    } as ReturnType<typeof usePersonas>);
    const { prefetchQuery } = renderControl();
    const trigger = screen.getByRole("button", { name: "Choose persona" });

    fireEvent.pointerEnter(trigger);
    await waitFor(() => expect(prefetchQuery).toHaveBeenCalledOnce());
    fireEvent.click(trigger);

    expect(screen.getByTestId("persona-picker-popover")).toBeInTheDocument();
    expect(screen.getByTestId("persona-menu-loading")).toBeInTheDocument();
  });

  it("shows a retryable error row instead of the empty No persona option", () => {
    const refetch = vi.fn();
    vi.mocked(usePersonas).mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: true,
      refetch,
    } as unknown as ReturnType<typeof usePersonas>);

    renderControl();
    fireEvent.click(screen.getByRole("button", { name: "Choose persona" }));

    expect(screen.getByText("Couldn't load personas.")).toBeInTheDocument();
    expect(screen.queryByRole("menuitemradio", { name: "No persona" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry personas" }));
    expect(refetch).toHaveBeenCalledOnce();
  });
});
