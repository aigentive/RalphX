import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { TicketAssigneeChip } from "./TicketAssigneeChip";

describe("TicketAssigneeChip", () => {
  it("renders initials only and exposes the assignee name as hover text", () => {
    render(<TicketAssigneeChip person={{ name: "Alex Developer" }} />);

    expect(screen.getByText("AD")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Alex Developer" })).toHaveAttribute(
      "title",
      "Alex Developer",
    );
    expect(screen.queryByText("Alex Developer")).not.toBeInTheDocument();
  });

  it("renders an avatar image when avatarUrl is present", () => {
    render(
      <TicketAssigneeChip
        person={{ name: "Alex Developer", avatarUrl: "https://example.com/a.png" }}
      />,
    );

    const img = document.querySelector("img");
    expect(img).not.toBeNull();
    expect(img?.getAttribute("src")).toBe("https://example.com/a.png");
    // Initials fallback is not rendered when an avatar exists.
    expect(screen.queryByText("AD")).not.toBeInTheDocument();
  });

  it("surfaces the email through the title tooltip", () => {
    const { container } = render(
      <TicketAssigneeChip person={{ name: "Alex Developer", email: "alex@example.com" }} />,
    );

    expect(container.querySelector("[title='Alex Developer · alex@example.com']")).not.toBeNull();
  });

  it("renders a muted placeholder when unassigned", () => {
    render(<TicketAssigneeChip person={null} />);
    expect(screen.getByText("Unassigned")).toBeInTheDocument();
  });

  it("honours a custom unassigned label", () => {
    render(<TicketAssigneeChip person={undefined} unassignedLabel="No owner" />);
    expect(screen.getByText("No owner")).toBeInTheDocument();
  });

  it("falls back to initials when the avatar image fails to load", () => {
    const { container } = render(
      <TicketAssigneeChip
        person={{ name: "Alex Developer", avatarUrl: "https://example.com/broken.png" }}
      />,
    );

    const img = container.querySelector("img");
    expect(img).not.toBeNull();
    expect(screen.queryByText("AD")).not.toBeInTheDocument();

    fireEvent.error(img as HTMLImageElement);

    expect(screen.getByText("AD")).toBeInTheDocument();
    expect(container.querySelector("img")).toBeNull();
  });
});
