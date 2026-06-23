import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { TicketLabels } from "./TicketLabels";

describe("TicketLabels", () => {
  it("renders nothing when there are no labels", () => {
    const { container } = render(<TicketLabels labels={[]} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders every label when within the max", () => {
    render(<TicketLabels labels={["backend", "linear"]} max={3} />);
    expect(screen.getByText("backend")).toBeInTheDocument();
    expect(screen.getByText("linear")).toBeInTheDocument();
    expect(screen.queryByText(/^\+/)).not.toBeInTheDocument();
  });

  it("collapses overflow into a +N chip listing the hidden labels", () => {
    const { container } = render(
      <TicketLabels labels={["a", "b", "c", "d", "e"]} max={3} />,
    );

    expect(screen.getByText("+2")).toBeInTheDocument();
    expect(screen.queryByText("d")).not.toBeInTheDocument();
    expect(container.querySelector("[title='d, e']")).not.toBeNull();
  });
});
