import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PersonaUnavailableNotice } from "./PersonaUnavailableNotice";

describe("PersonaUnavailableNotice", () => {
  it("shows the backend reason and delegates both recovery actions", () => {
    const onRemoveAndRetry = vi.fn();
    const onOpenPersonas = vi.fn();
    render(
      <PersonaUnavailableNotice
        message="Reviewer Voice was archived"
        onRemoveAndRetry={onRemoveAndRetry}
        onOpenPersonas={onOpenPersonas}
      />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Persona unavailable: Reviewer Voice was archived",
    );
    fireEvent.click(screen.getByRole("button", { name: "Remove persona and retry" }));
    fireEvent.click(screen.getByRole("button", { name: "Manage personas" }));

    expect(onRemoveAndRetry).toHaveBeenCalledOnce();
    expect(onOpenPersonas).toHaveBeenCalledOnce();
  });
});
