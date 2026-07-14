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

function PreparingConfirmationHarness({
  onPrepare,
}: {
  onPrepare: () => Promise<{ description: string }>;
}) {
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();

  return (
    <>
      <button
        type="button"
        onClick={() => {
          void confirm({
            title: "Start Workspace Review?",
            description: "Checking the current review target…",
            confirmText: "Start review",
            prepare: onPrepare,
          });
        }}
      >
        Open prepared confirmation
      </button>
      <ConfirmationDialog {...confirmationDialogProps} />
    </>
  );
}

describe("useConfirmation", () => {
  it("opens immediately and keeps Confirm disabled until async preparation supplies its copy", async () => {
    let finishPrepare!: (value: { description: string }) => void;
    const onPrepare = vi.fn(
      () =>
        new Promise<{ description: string }>((resolve) => {
          finishPrepare = resolve;
        }),
    );
    const user = userEvent.setup();

    render(<PreparingConfirmationHarness onPrepare={onPrepare} />);

    await user.click(
      screen.getByRole("button", { name: "Open prepared confirmation" }),
    );
    const dialog = await screen.findByRole("alertdialog");

    expect(onPrepare).toHaveBeenCalledTimes(1);
    expect(
      within(dialog).getByText("Checking the current review target…"),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole("button", { name: "Preparing..." }),
    ).toBeDisabled();

    finishPrepare({ description: "GitHub auto-merge will be temporarily paused." });

    await waitFor(() => {
      expect(
        within(dialog).getByText("GitHub auto-merge will be temporarily paused."),
      ).toBeInTheDocument();
      expect(
        within(dialog).getByRole("button", { name: "Start review" }),
      ).toBeEnabled();
    });
  });

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
