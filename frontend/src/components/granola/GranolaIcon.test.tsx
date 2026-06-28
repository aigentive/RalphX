import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { GranolaIcon } from "./GranolaIcon";

describe("GranolaIcon", () => {
  it("renders the Granola symbol inline so rail colors can drive it", () => {
    const { container } = render(<GranolaIcon className="h-5 w-5 text-current" />);

    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    expect(svg).toHaveAttribute("viewBox", "0 0 1024 1024");
    expect(svg?.getAttribute("style") ?? "").not.toContain("background-image");
  });
});
