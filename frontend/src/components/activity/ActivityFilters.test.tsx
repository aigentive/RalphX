import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";

import { StatusFilter, RoleFilter } from "./ActivityFilters";

describe("ActivityFilters badges", () => {
  it("renders the selection count badge on StatusFilter when statuses are selected", () => {
    render(<StatusFilter selectedStatuses={["completed", "failed"]} onChange={vi.fn()} />);
    expect(screen.getByText("Status")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
  });

  it("hides the count badge on StatusFilter when no statuses are selected", () => {
    render(<StatusFilter selectedStatuses={[]} onChange={vi.fn()} />);
    expect(screen.getByText("Status")).toBeInTheDocument();
    // No count badge when selection is empty.
    expect(screen.queryByText("0")).toBeNull();
  });

  it("renders the selection count badge on RoleFilter when roles are selected", () => {
    render(<RoleFilter selectedRoles={["worker"]} onChange={vi.fn()} />);
    expect(screen.getByText("Role")).toBeInTheDocument();
    expect(screen.getByText("1")).toBeInTheDocument();
  });

  it("hides the count badge on RoleFilter when no roles are selected", () => {
    render(<RoleFilter selectedRoles={[]} onChange={vi.fn()} />);
    expect(screen.getByText("Role")).toBeInTheDocument();
  });
});
