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
    fireEvent.pointerMove(badge);
    expect(
      await screen.findByRole("tooltip", {
        name: "design-voice · v2 — applied to this run",
      }),
    ).toBeInTheDocument();
  });

  it("opens a details popover with the run outcome and persona deep link", () => {
    renderBadge({ personaId: "persona-1" });

    fireEvent.click(screen.getByTestId("persona-run-badge"));
    const details = screen.getByTestId("persona-run-badge-details");
    expect(details).toHaveTextContent("design-voice");
    expect(details).toHaveTextContent("Applied to this run.");
    expect(
      screen.getByRole("button", { name: "Open persona" }),
    ).toBeInTheDocument();
  });

  it("shows the full skip reason and no deep link without a persona id", () => {
    renderBadge({
      personaInjected: false,
      skippedReason: "native_agent_flag",
    });

    fireEvent.click(screen.getByTestId("persona-run-badge"));
    expect(screen.getByTestId("persona-run-badge-details")).toHaveTextContent(
      "Native agent mode does not support personas",
    );
    expect(
      screen.queryByRole("button", { name: "Open persona" }),
    ).not.toBeInTheDocument();
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
