import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  usePersonaArtifactHistory,
  usePersonaArtifactVersion,
} from "@/hooks/usePersonaArtifact";

import { PersonaVersionHistory } from "./PersonaVersionHistory";

vi.mock("@/hooks/usePersonaArtifact", () => ({
  usePersonaArtifactHistory: vi.fn(),
  usePersonaArtifactVersion: vi.fn(),
}));

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

function renderHistory(selectedVersion: number | null = null) {
  return render(
    <PersonaVersionHistory
      artifactId="artifact-current"
      currentContent="Current content"
      selectedVersion={selectedVersion}
      onSelectedVersionChange={vi.fn()}
    />,
  );
}

describe("PersonaVersionHistory", () => {
  beforeEach(() => {
    vi.mocked(usePersonaArtifactHistory).mockReturnValue({
      data: history,
      isError: false,
    } as ReturnType<typeof usePersonaArtifactHistory>);
    vi.mocked(usePersonaArtifactVersion).mockReturnValue({
      data: undefined,
      isPending: false,
      isError: false,
    } as ReturnType<typeof usePersonaArtifactVersion>);
  });

  it("shows a history error instead of hiding the version control as empty", () => {
    vi.mocked(usePersonaArtifactHistory).mockReturnValue({
      data: undefined,
      isError: true,
    } as ReturnType<typeof usePersonaArtifactHistory>);

    renderHistory();

    expect(screen.getByText("Couldn't load version history.")).toBeInTheDocument();
    expect(screen.queryByLabelText("Persona version")).not.toBeInTheDocument();
    expect(screen.getByText("Current content")).toBeInTheDocument();
  });

  it("keeps an empty successful history distinct from a history error", () => {
    vi.mocked(usePersonaArtifactHistory).mockReturnValue({
      data: [],
      isError: false,
    } as ReturnType<typeof usePersonaArtifactHistory>);

    renderHistory();

    expect(screen.queryByText("Couldn't load version history.")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Persona version")).not.toBeInTheDocument();
  });

  it("shows a version-content error instead of no-inline-content", () => {
    vi.mocked(usePersonaArtifactVersion).mockReturnValue({
      data: undefined,
      isPending: false,
      isError: true,
    } as ReturnType<typeof usePersonaArtifactVersion>);

    renderHistory(1);

    expect(screen.getByText("Couldn't load this persona version.")).toBeInTheDocument();
    expect(screen.queryByText("This version has no inline content.")).not.toBeInTheDocument();
  });

  it("keeps successful non-inline content distinct from a version error", () => {
    vi.mocked(usePersonaArtifactVersion).mockReturnValue({
      data: {
        id: "artifact-old",
        name: "Support Voice",
        artifact_type: "persona",
        content: { type: "file", path: "/artifact/content.md" },
        created_at: "2026-07-17T09:00:00Z",
        created_by: "agent",
        version: 1,
        bucket_id: "persona-library",
        task_id: null,
        process_id: null,
        derived_from: [],
      },
      isPending: false,
      isError: false,
    } as ReturnType<typeof usePersonaArtifactVersion>);

    renderHistory(1);

    expect(screen.getByText("This version has no inline content.")).toBeInTheDocument();
    expect(screen.queryByText("Couldn't load this persona version.")).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Persona version"), {
      target: { value: "current" },
    });
  });
});
