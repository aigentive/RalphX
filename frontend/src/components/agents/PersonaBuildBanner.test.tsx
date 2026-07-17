import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { PersonaBuildBanner } from "./PersonaBuildBanner";

describe("PersonaBuildBanner", () => {
  it.each([
    [null, null, "Building a Global persona · private workspace"],
    ["RalphX", null, "Building a persona for RalphX"],
    [null, "Reviewer Voice", "Refining 'Reviewer Voice'"],
  ])("renders the expected build context", (projectName, sourcePersonaName, title) => {
    render(
      <PersonaBuildBanner
        projectName={projectName}
        sourcePersonaName={sourcePersonaName}
      />,
    );
    expect(screen.getByTestId("persona-build-banner")).toHaveTextContent(title);
  });
});
