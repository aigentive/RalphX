import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { BranchBasePicker } from "./BranchBasePicker";
import type { BranchBaseOption } from "./branchBaseOptions";

const options: BranchBaseOption[] = [
  {
    key: "project_default::main",
    label: "main",
    detail: "Project default",
    source: "project",
    selection: { kind: "project_default", ref: "main", displayName: "main" },
  },
  {
    key: "local_branch::feature/x",
    label: "feature/x",
    detail: "Local branch",
    source: "local",
    selection: { kind: "local_branch", ref: "feature/x", displayName: "feature/x" },
  },
  {
    key: "current_branch::feature/current",
    label: "Current branch (feature/current)",
    detail: "Currently checked out in the project root",
    source: "current",
    selection: {
      kind: "current_branch",
      ref: "feature/current",
      displayName: "Current branch (feature/current)",
    },
  },
];

describe("BranchBasePicker", () => {
  it("renders the placeholder when no value is selected", () => {
    render(
      <BranchBasePicker
        value=""
        onValueChange={vi.fn()}
        options={options}
        placeholder="Select base"
        testId="picker"
      />,
    );
    expect(screen.getByText("Select base")).toBeInTheDocument();
  });

  it("renders the selected option label", () => {
    render(
      <BranchBasePicker
        value="project_default::main"
        onValueChange={vi.fn()}
        options={options}
        placeholder="Select base"
        testId="picker"
      />,
    );
    expect(screen.getByText("main")).toBeInTheDocument();
  });

  it("opens the popover and lists filtered options", async () => {
    const user = userEvent.setup();
    const onValueChange = vi.fn();
    render(
      <BranchBasePicker
        value=""
        onValueChange={onValueChange}
        options={options}
        placeholder="Select base"
        testId="picker"
      />,
    );

    await user.click(screen.getByTestId("picker"));
    // Both options listed in popover.
    expect(screen.getAllByText("main").length).toBeGreaterThan(0);
    expect(screen.getByText("feature/x")).toBeInTheDocument();

    // Filter via search.
    const search = screen.getByPlaceholderText(/Search branches/i);
    fireEvent.change(search, { target: { value: "feature" } });
    expect(screen.queryByText("Project default")).toBeNull();
    expect(screen.getByText("feature/x")).toBeInTheDocument();

    await user.click(screen.getByText("feature/x"));
    expect(onValueChange).toHaveBeenCalledWith("local_branch::feature/x");
  });

  it("keeps cached options visible while a refresh is loading", async () => {
    const user = userEvent.setup();
    render(
      <BranchBasePicker
        value="local_branch::feature/x"
        onValueChange={vi.fn()}
        options={options}
        placeholder="Select base"
        testId="picker"
        isLoading
      />,
    );

    expect(screen.getByTestId("picker")).toHaveTextContent("feature/x");

    await user.click(screen.getByTestId("picker"));

    expect(screen.getByText("Refreshing branches...")).toBeInTheDocument();
    expect(screen.getAllByText("feature/x").length).toBeGreaterThan(0);
  });

  it("renders the isolated branch switch and reports toggle changes", async () => {
    const user = userEvent.setup();
    const onIsolatedBranchChange = vi.fn();
    render(
      <BranchBasePicker
        value="local_branch::feature/x"
        onValueChange={vi.fn()}
        options={options}
        placeholder="Select base"
        testId="picker"
        isolatedBranch={false}
        onIsolatedBranchChange={onIsolatedBranchChange}
      />,
    );

    await user.click(screen.getByTestId("picker"));

    expect(screen.getByText("Isolated branch")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /About isolated branch/i }),
    ).toBeInTheDocument();
    await user.hover(screen.getByRole("button", { name: /About isolated branch/i }));
    expect(
      (await screen.findAllByText(/Starts isolated by default/i)).length,
    ).toBeGreaterThan(0);
    const isolatedSwitch = screen.getByRole("switch", {
      name: /Use isolated branch/i,
    });
    expect(isolatedSwitch).toHaveAttribute("aria-checked", "false");

    await user.click(isolatedSwitch);
    expect(onIsolatedBranchChange).toHaveBeenCalledWith(true);
  });

  it("can keep the popover open after selecting an option", async () => {
    const user = userEvent.setup();
    render(
      <BranchBasePicker
        value=""
        onValueChange={vi.fn()}
        options={options}
        placeholder="Select base"
        testId="picker"
        closeOnSelect={false}
        isolatedBranch={false}
        onIsolatedBranchChange={vi.fn()}
      />,
    );

    await user.click(screen.getByTestId("picker"));
    await user.click(screen.getByText("feature/x"));

    expect(screen.getByPlaceholderText(/Search branches/i)).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: /Use isolated branch/i })).toBeInTheDocument();
  });

  it("disables the isolated branch switch for project default selections", async () => {
    const user = userEvent.setup();
    render(
      <BranchBasePicker
        value="project_default::main"
        onValueChange={vi.fn()}
        options={options}
        placeholder="Select base"
        testId="picker"
        isolatedBranch
        isolatedBranchDisabled
        onIsolatedBranchChange={vi.fn()}
      />,
    );

    await user.click(screen.getByTestId("picker"));

    const isolatedSwitch = screen.getByRole("switch", {
      name: /Use isolated branch/i,
    });
    expect(isolatedSwitch).toBeDisabled();
    expect(isolatedSwitch).toHaveAttribute("aria-checked", "true");
  });

  it("disables the isolated branch switch for current branch selections", async () => {
    const user = userEvent.setup();
    render(
      <BranchBasePicker
        value="current_branch::feature/current"
        onValueChange={vi.fn()}
        options={options}
        placeholder="Select base"
        testId="picker"
        isolatedBranch
        isolatedBranchDisabled
        onIsolatedBranchChange={vi.fn()}
      />,
    );

    await user.click(screen.getByTestId("picker"));

    const isolatedSwitch = screen.getByRole("switch", {
      name: /Use isolated branch/i,
    });
    expect(isolatedSwitch).toBeDisabled();
    expect(isolatedSwitch).toHaveAttribute("aria-checked", "true");
  });

  it("switches to pull request results and selects a PR head branch option", async () => {
    const user = userEvent.setup();
    const onValueChange = vi.fn();
    const pullRequestOptions: BranchBaseOption[] = [
      {
        key: "pull_request:42:feature/pr-picker",
        label: "#42 Add PR picker",
        detail: "feature/pr-picker -> main",
        source: "pull_request",
        selection: {
          kind: "local_branch",
          ref: "feature/pr-picker",
          displayName: "PR #42: Add PR picker",
        },
      },
    ];

    render(
      <BranchBasePicker
        value=""
        onValueChange={onValueChange}
        options={options}
        pullRequestOptions={pullRequestOptions}
        enablePullRequests
        placeholder="Select base"
        testId="picker"
      />,
    );

    await user.click(screen.getByTestId("picker"));
    await user.click(screen.getByRole("tab", { name: /PRs/i }));

    expect(screen.getByPlaceholderText(/Search pull requests/i)).toBeInTheDocument();
    expect(screen.getByText("#42 Add PR picker")).toBeInTheDocument();

    await user.click(screen.getByText("#42 Add PR picker"));
    expect(onValueChange).toHaveBeenCalledWith("pull_request:42:feature/pr-picker");
  });

  it("debounces pull request searches and renders pull request status", async () => {
    const user = userEvent.setup();
    const onPullRequestSearch = vi.fn();

    render(
      <BranchBasePicker
        value=""
        onValueChange={vi.fn()}
        options={options}
        enablePullRequests
        isLoadingPullRequests
        pullRequestMessage="Unable to search pull requests"
        onPullRequestSearch={onPullRequestSearch}
        placeholder="Select base"
        testId="picker"
      />,
    );

    await user.click(screen.getByTestId("picker"));
    await user.click(screen.getByRole("tab", { name: /PRs/i }));

    expect(screen.getByText("Searching pull requests...")).toBeInTheDocument();
    expect(screen.getByText("Unable to search pull requests")).toBeInTheDocument();

    await user.type(screen.getByPlaceholderText(/Search pull requests/i), "fix");
    await waitFor(() => {
      expect(onPullRequestSearch).toHaveBeenLastCalledWith("fix");
    });
  });
});
