import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";

import { TicketLabelEditor } from "./TicketLabelEditor";

function renderEditor(props: Partial<React.ComponentProps<typeof TicketLabelEditor>> = {}) {
  const onSetLabels = props.onSetLabels ?? vi.fn();
  render(
    <TooltipProvider>
      <TicketLabelEditor
        provider={props.provider ?? "jira"}
        labels={props.labels ?? []}
        {...(props.labelOptions ? { labelOptions: props.labelOptions } : {})}
        {...(props.isLabelOptionsLoading !== undefined
          ? { isLabelOptionsLoading: props.isLabelOptionsLoading }
          : {})}
        {...(props.isLabelPending !== undefined ? { isLabelPending: props.isLabelPending } : {})}
        {...(props.disabledReason !== undefined ? { disabledReason: props.disabledReason } : {})}
        onSetLabels={onSetLabels}
      />
    </TooltipProvider>,
  );
  return { onSetLabels };
}

describe("TicketLabelEditor (Jira free-text)", () => {
  it("adds a typed label to the full set via Enter", () => {
    const { onSetLabels } = renderEditor({ provider: "jira", labels: ["bug"] });
    const input = screen.getByLabelText("Add a label");
    fireEvent.change(input, { target: { value: "frontend" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSetLabels).toHaveBeenCalledWith(["bug", "frontend"]);
  });

  it("adds a typed label via the Add button", () => {
    const { onSetLabels } = renderEditor({ provider: "jira", labels: [] });
    fireEvent.change(screen.getByLabelText("Add a label"), { target: { value: "infra" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(onSetLabels).toHaveBeenCalledWith(["infra"]);
  });

  it("rejects labels with spaces and shows the Jira message", () => {
    const { onSetLabels } = renderEditor({ provider: "jira", labels: [] });
    const input = screen.getByLabelText("Add a label");
    fireEvent.change(input, { target: { value: "needs work" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSetLabels).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("Jira labels can't contain spaces");
  });

  it("does not add a case-insensitive duplicate label", () => {
    const { onSetLabels } = renderEditor({ provider: "jira", labels: ["Bug"] });
    fireEvent.change(screen.getByLabelText("Add a label"), { target: { value: "bug" } });
    fireEvent.keyDown(screen.getByLabelText("Add a label"), { key: "Enter" });
    expect(onSetLabels).not.toHaveBeenCalled();
  });

  it("removes a label via the chip remove button", () => {
    const { onSetLabels } = renderEditor({ provider: "jira", labels: ["bug", "frontend"] });
    fireEvent.click(screen.getByLabelText("Remove label frontend"));
    expect(onSetLabels).toHaveBeenCalledWith(["bug"]);
  });
});

describe("TicketLabelEditor (Linear pick-list)", () => {
  it("appends a selected team label and filters out already-applied options", () => {
    const { onSetLabels } = renderEditor({
      provider: "linear",
      labels: ["bug"],
      labelOptions: [
        { id: "label-bug", name: "bug" },
        { id: "label-feature", name: "feature" },
      ],
    });
    // The applied "bug" option is not offered.
    expect(screen.queryByRole("option", { name: "bug" })).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Add a label"), { target: { value: "feature" } });
    expect(onSetLabels).toHaveBeenCalledWith(["bug", "feature"]);
  });

  it("does not offer free-text label entry in Linear mode", () => {
    renderEditor({
      provider: "linear",
      labels: [],
      labelOptions: [{ id: "label-bug", name: "bug" }],
    });
    expect(screen.queryByRole("textbox", { name: "Add a label" })).not.toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Add a label" })).toBeInTheDocument();
  });

  it("shows a hint when there are no selectable team labels", () => {
    renderEditor({ provider: "linear", labels: [], labelOptions: [] });
    expect(screen.getByText(/no more team labels/i)).toBeInTheDocument();
  });

  it("shows a loading hint while team labels load", () => {
    renderEditor({ provider: "linear", labels: [], isLabelOptionsLoading: true });
    expect(screen.getByText(/loading team labels/i)).toBeInTheDocument();
  });
});

describe("TicketLabelEditor gating", () => {
  it("renders read-only with a reason and no controls when disabled", () => {
    const onSetLabels = vi.fn();
    renderEditor({
      provider: "jira",
      labels: ["bug"],
      disabledReason: "Label editing is not available for this provider.",
      onSetLabels,
    });
    expect(screen.getByText("bug")).toBeInTheDocument();
    expect(screen.getByText("Label editing is not available for this provider.")).toBeInTheDocument();
    expect(screen.queryByLabelText("Add a label")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Remove label bug")).not.toBeInTheDocument();
  });

  it("disables controls while a label write is pending", () => {
    renderEditor({ provider: "jira", labels: ["bug"], isLabelPending: true });
    expect(screen.getByLabelText("Add a label")).toBeDisabled();
    expect(screen.getByLabelText("Remove label bug")).toBeDisabled();
  });
});
