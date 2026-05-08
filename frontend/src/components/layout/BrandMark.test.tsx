import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { useThemeStore } from "@/stores/themeStore";
import { BrandMark } from "./BrandMark";

describe("BrandMark", () => {
  beforeEach(() => {
    useThemeStore.setState({ theme: "light" });
  });

  it("renders the v27 light logo colors as literal SVG fills", () => {
    render(<BrandMark />);

    const svg = screen.getByRole("img", { name: "RalphX" });
    expect(svg).toHaveClass("h-[44px]", "w-[44px]");
    expect(svg.querySelector('rect[width="1254"]')).toHaveAttribute("fill", "#DEDEE2");
    expect(svg.querySelector("stop:first-child")).toHaveAttribute("stop-color", "#F0F0F2");
    expect(svg.querySelector("path")).toHaveAttribute("fill", "#FA4F19");
  });

  it("switches to high-contrast logo colors without CSS var fallback fills", () => {
    useThemeStore.setState({ theme: "high-contrast" });

    render(<BrandMark />);

    const svg = screen.getByRole("img", { name: "RalphX" });
    expect(svg.querySelector('rect[width="1254"]')).toHaveAttribute("fill", "#1A1A1A");
    expect(svg.innerHTML).not.toContain("var(--brand");
  });
});
