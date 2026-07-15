import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";

import { PersonaRunBadge } from "./PersonaRunBadge";

function renderBadge(
  props: Partial<React.ComponentProps<typeof PersonaRunBadge>> = {},
) {
  return render(
    <TooltipProvider delayDuration={0}>
      <PersonaRunBadge
        enabled
        personaSlug="design-voice"
        personaVersion={2}
        personaInjected
        skippedReason={null}
        {...props}
      />
    </TooltipProvider>,
  );
}

describe("PersonaRunBadge", () => {
  it("renders an applied persona slug with its version in the tooltip", async () => {
    renderBadge();

    const badge = screen.getByTestId("persona-run-badge");
    expect(badge).toHaveTextContent("design-voice");
    expect(badge.tagName).not.toBe("BUTTON");
    fireEvent.pointerMove(badge);
    expect(
      await screen.findByRole("tooltip", {
        name: "design-voice · v2 — applied to this run",
      }),
    ).toBeInTheDocument();
  });

  it("renders a skipped persona with a human-readable native-agent reason", async () => {
    renderBadge({
      personaInjected: false,
      skippedReason: "native_agent_flag",
    });

    const badge = screen.getByTestId("persona-run-badge");
    expect(badge).toHaveTextContent("design-voice not applied");
    fireEvent.pointerMove(badge);
    expect(
      await screen.findByRole("tooltip", {
        name: "Native agent mode does not support personas",
      }),
    ).toBeInTheDocument();
  });

  it("renders nothing when attribution is absent or the feature flag is off", () => {
    const { rerender } = renderBadge({ personaSlug: null, personaInjected: null });
    expect(screen.queryByTestId("persona-run-badge")).not.toBeInTheDocument();

    rerender(
      <TooltipProvider delayDuration={0}>
        <PersonaRunBadge
          enabled={false}
          personaSlug="design-voice"
          personaVersion={2}
          personaInjected
          skippedReason={null}
        />
      </TooltipProvider>,
    );
    expect(screen.queryByTestId("persona-run-badge")).not.toBeInTheDocument();
  });
});
