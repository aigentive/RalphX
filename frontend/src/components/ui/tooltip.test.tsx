import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "./tooltip";

describe("TooltipContent", () => {
  it("uses explicit theme token styles for app tooltip chrome", async () => {
    const user = userEvent.setup();

    render(
      <TooltipProvider delayDuration={0}>
        <Tooltip>
          <TooltipTrigger asChild>
            <button type="button">Hover me</button>
          </TooltipTrigger>
          <TooltipContent>Tooltip text</TooltipContent>
        </Tooltip>
      </TooltipProvider>,
    );

    await user.hover(screen.getByRole("button", { name: "Hover me" }));

    await screen.findByRole("tooltip");

    const tooltip = Array.from(
      document.querySelectorAll<HTMLElement>("[data-side][data-align]"),
    ).find((element) => element.textContent?.includes("Tooltip text"));
    const style = tooltip?.getAttribute("style") ?? "";

    expect(tooltip).toBeTruthy();
    expect(style).toContain("background-color: var(--tooltip-bg)");
    expect(style).toContain("border-color: var(--tooltip-border)");
    expect(style).toContain("border-style: solid");
    expect(style).toContain("border-width: 1px");
    expect(style).toContain("box-shadow: var(--tooltip-shadow)");
    expect(style).toContain("color: var(--tooltip-text)");
  });
});
