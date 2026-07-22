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
  recoverFromPrepareError,
}: {
  onPrepare: () => Promise<{ description: string }>;
  recoverFromPrepareError?: (error: unknown) =>
    | {
        description: string;
        confirmDisabled: boolean;
      }
    | null;
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
            recoverFromPrepareError,
          });
        }}
      >
        Open prepared confirmation
      </button>
      <ConfirmationDialog {...confirmationDialogProps} />
    </>
  );
}

function RecoveringConfirmationHarness({
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
            title: "Start Workspace Review?",
            description: "Ready to start.",
            confirmText: "Start review",
            pendingText: "Starting...",
            onConfirm,
            recoverFromError: () => ({
              description: "Complete or abort the Git operation before retrying.",
              confirmDisabled: true,
            }),
          });
        }}
      >
        Open recoverable confirmation
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

  it("shows a cancellable preparation error without an endless loading state", async () => {
    const onPrepare = vi.fn().mockRejectedValue(new Error("preview unavailable"));
    const user = userEvent.setup();

    render(<PreparingConfirmationHarness onPrepare={onPrepare} />);
    await user.click(
      screen.getByRole("button", { name: "Open prepared confirmation" }),
    );
    const dialog = await screen.findByRole("alertdialog");

    await waitFor(() => {
      expect(
        within(dialog).getByText("Could not prepare this action. Cancel and try again."),
      ).toBeInTheDocument();
    });
    expect(within(dialog).queryByRole("button", { name: "Preparing..." })).toBeNull();
    expect(within(dialog).getByRole("button", { name: "Start review" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeEnabled();
  });

  it("renders a caller-specific preparation block and keeps confirmation disabled", async () => {
    const error = new Error("unfinished git operation");
    const onPrepare = vi.fn().mockRejectedValue(error);
    const recoverFromPrepareError = vi.fn().mockReturnValue({
      description:
        "Resolve conflicts and complete or abort the merge or rebase before retrying.",
      confirmDisabled: true,
    });
    const user = userEvent.setup();

    render(
      <PreparingConfirmationHarness
        onPrepare={onPrepare}
        recoverFromPrepareError={recoverFromPrepareError}
      />,
    );
    await user.click(
      screen.getByRole("button", { name: "Open prepared confirmation" }),
    );
    const dialog = await screen.findByRole("alertdialog");

    await waitFor(() => {
      expect(
        within(dialog).getByText(
          "Resolve conflicts and complete or abort the merge or rebase before retrying.",
        ),
      ).toBeInTheDocument();
    });
    expect(recoverFromPrepareError).toHaveBeenCalledWith(error);
    expect(within(dialog).getByRole("button", { name: "Start review" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeEnabled();

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("keeps the generic disabled preparation fallback when recovery mapping fails", async () => {
    const onPrepare = vi.fn().mockRejectedValue(new Error("preview unavailable"));
    const recoverFromPrepareError = vi.fn(() => {
      throw new Error("mapper failed");
    });
    const user = userEvent.setup();

    render(
      <PreparingConfirmationHarness
        onPrepare={onPrepare}
        recoverFromPrepareError={recoverFromPrepareError}
      />,
    );
    await user.click(
      screen.getByRole("button", { name: "Open prepared confirmation" }),
    );
    const dialog = await screen.findByRole("alertdialog");

    await waitFor(() => {
      expect(
        within(dialog).getByText("Could not prepare this action. Cancel and try again."),
      ).toBeInTheDocument();
    });
    expect(within(dialog).getByRole("button", { name: "Start review" })).toBeDisabled();
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

  it("settles a recovered blocked submission into a cancellable disabled state", async () => {
    const onConfirm = vi.fn().mockRejectedValue(new Error("target blocked"));
    const user = userEvent.setup();

    render(<RecoveringConfirmationHarness onConfirm={onConfirm} />);
    await user.click(
      screen.getByRole("button", { name: "Open recoverable confirmation" }),
    );
    const dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Start review" }));

    await waitFor(() => {
      expect(
        within(dialog).getByText("Complete or abort the Git operation before retrying."),
      ).toBeInTheDocument();
      expect(within(dialog).getByRole("button", { name: "Start review" })).toBeDisabled();
    });
    expect(within(dialog).queryByRole("button", { name: "Starting..." })).toBeNull();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeEnabled();

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });
});
