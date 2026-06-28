import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { GitHubMarkIcon } from "./GitHubMarkIcon";

describe("GitHubMarkIcon", () => {
  it("renders inline SVG instead of a CSS mask image", () => {
    const { container } = render(<GitHubMarkIcon className="h-5 w-5 text-current" />);

    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    expect(svg).toHaveAttribute("viewBox", "0 0 98 96");
    expect(svg?.getAttribute("style") ?? "").not.toContain("mask-image");
    expect(svg?.getAttribute("style") ?? "").not.toContain("background-image");
  });
});
