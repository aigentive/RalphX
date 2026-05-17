import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { useConfirmation } from "./useConfirmation";

function ConfirmationHarness({
  onConfirm,
}: {
  onConfirm: () => Promise<void>;
}) {
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();

  return (
    <>
      <button
        type="button"
        onClick={() => {
          void confirm({
            title: "Run backend action?",
            description: "This starts an async backend action.",
            confirmText: "Run action",
            pendingText: "Running...",
            onConfirm,
          });
        }}
      >
        Open confirmation
      </button>
      <ConfirmationDialog {...confirmationDialogProps} />
    </>
  );
}

describe("useConfirmation", () => {
  it("keeps the dialog open with disabled actions while async confirmation is submitting", async () => {
    let finishConfirm!: () => void;
    const onConfirm = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finishConfirm = resolve;
        }),
    );
    const user = userEvent.setup();

    render(<ConfirmationHarness onConfirm={onConfirm} />);

    await user.click(screen.getByRole("button", { name: "Open confirmation" }));
    const dialog = await screen.findByRole("alertdialog");

    await user.click(within(dialog).getByRole("button", { name: "Run action" }));

    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(
      within(dialog).getByRole("button", { name: "Running..." }),
    ).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();

    finishConfirm();

    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    });
  });
});
