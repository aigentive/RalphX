import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
            body: <input aria-label="Runtime model" defaultValue="sonnet" />,
            onConfirm,
            recoverFromError: () => ({
              description: "Complete or abort the Git operation before retrying.",
              bodyDisabled: true,
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

function SupersedingConfirmationHarness({
  firstAction,
  secondAction,
  onFirstResult,
  onSecondResult,
}: {
  firstAction: () => Promise<void>;
  secondAction: () => Promise<void>;
  onFirstResult: (result: boolean) => void;
  onSecondResult: (result: boolean) => void;
}) {
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();

  return (
    <>
      <button
        type="button"
        onClick={() => {
          void confirm({
            title: "First confirmation",
            description: "The first action is still running.",
            confirmText: "Confirm first",
            onConfirm: firstAction,
          }).then(onFirstResult);
        }}
      >
        Open first
      </button>
      <button
        type="button"
        data-testid="open-second-confirmation"
        onClick={() => {
          void confirm({
            title: "Second confirmation",
            description: "This request supersedes the first dialog.",
            confirmText: "Confirm second",
            onConfirm: secondAction,
          }).then(onSecondResult);
        }}
      >
        Open second
      </button>
      <ConfirmationDialog {...confirmationDialogProps} />
    </>
  );
}

function IntentConfirmationHarness({
  onConfirm,
  onIntent,
}: {
  onConfirm: () => Promise<void>;
  onIntent: () => void;
}) {
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();

  return (
    <>
      <button
        type="button"
        onClick={() => {
          void confirm({
            title: "Start Workspace Review?",
            description: "Review details are still loading.",
            confirmText: "Start review",
            closeOnConfirm: true,
            onConfirm,
            onIntent,
          });
        }}
      >
        Open intent confirmation
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
    const runtimeModel = within(dialog).getByRole("textbox", {
      name: "Runtime model",
    });
    await user.click(within(dialog).getByRole("button", { name: "Start review" }));

    expect(runtimeModel).toBeDisabled();

    await waitFor(() => {
      expect(
        within(dialog).getByText("Complete or abort the Git operation before retrying."),
      ).toBeInTheDocument();
      expect(within(dialog).getByRole("button", { name: "Start review" })).toBeEnabled();
    });
    expect(runtimeModel).toBeDisabled();
    expect(within(dialog).queryByRole("button", { name: "Starting..." })).toBeNull();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeEnabled();

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("keeps a newer dialog isolated from a superseded submission", async () => {
    let finishFirst!: () => void;
    const firstAction = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finishFirst = resolve;
        }),
    );
    const secondAction = vi.fn().mockResolvedValue(undefined);
    const onFirstResult = vi.fn();
    const onSecondResult = vi.fn();
    const user = userEvent.setup();

    render(
      <SupersedingConfirmationHarness
        firstAction={firstAction}
        secondAction={secondAction}
        onFirstResult={onFirstResult}
        onSecondResult={onSecondResult}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Open first" }));
    await user.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Confirm first",
      }),
    );
    fireEvent.click(screen.getByTestId("open-second-confirmation"));

    await waitFor(() => expect(onFirstResult).toHaveBeenCalledWith(false));
    const secondDialog = await screen.findByRole("alertdialog");
    expect(within(secondDialog).getByText("Second confirmation")).toBeInTheDocument();

    finishFirst();
    await Promise.resolve();
    expect(screen.getByRole("alertdialog")).toBe(secondDialog);
    expect(onSecondResult).not.toHaveBeenCalled();

    await user.click(
      within(secondDialog).getByRole("button", { name: "Confirm second" }),
    );
    await waitFor(() => expect(onSecondResult).toHaveBeenCalledWith(true));
  });

  it("captures intent and closes before its detached action settles", async () => {
    let finishConfirm!: () => void;
    const onConfirm = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finishConfirm = resolve;
        }),
    );
    const onIntent = vi.fn();
    const user = userEvent.setup();

    render(<IntentConfirmationHarness onConfirm={onConfirm} onIntent={onIntent} />);
    await user.click(
      screen.getByRole("button", { name: "Open intent confirmation" }),
    );
    await user.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Start review",
      }),
    );

    expect(onIntent).toHaveBeenCalledOnce();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    await waitFor(() => expect(onConfirm).toHaveBeenCalledOnce());

    finishConfirm();
  });
});
