import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { TicketAssigneeChip } from "./TicketAssigneeChip";

describe("TicketAssigneeChip", () => {
  it("renders the assignee name with initials when no avatar is provided", () => {
    render(<TicketAssigneeChip person={{ name: "Adrian Demian" }} />);

    expect(screen.getByText("Adrian Demian")).toBeInTheDocument();
    expect(screen.getByText("AD")).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });

  it("renders an avatar image when avatarUrl is present", () => {
    render(
      <TicketAssigneeChip
        person={{ name: "Adrian Demian", avatarUrl: "https://example.com/a.png" }}
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
      <TicketAssigneeChip person={{ name: "Adrian Demian", email: "adrian@example.com" }} />,
    );

    expect(container.querySelector("[title='Adrian Demian · adrian@example.com']")).not.toBeNull();
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
        person={{ name: "Adrian Demian", avatarUrl: "https://example.com/broken.png" }}
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
