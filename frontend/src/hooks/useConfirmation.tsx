/* eslint-disable react-refresh/only-export-components */
import { useState, useRef, useCallback, useMemo } from "react";
import { Loader2 } from "lucide-react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";

export interface ConfirmOptions {
  title: string;
  description: string;
  confirmText?: string;
  pendingText?: string;
  cancelText?: string;
  variant?: "default" | "destructive";
  onConfirm?: () => Promise<unknown> | unknown;
  /** Runs after the dialog shell opens; return copy updates for the same dialog. */
  prepare?: () =>
    | Promise<Partial<Pick<ConfirmOptions, "title" | "description" | "confirmText">>>
    | Partial<Pick<ConfirmOptions, "title" | "description" | "confirmText">>;
  /** Return updated copy after a recoverable action error to keep the dialog open for reconfirmation. */
  recoverFromError?: (
    error: unknown,
  ) =>
    | Promise<
        Partial<Pick<ConfirmOptions, "title" | "description" | "confirmText">> | null
      >
    | Partial<Pick<ConfirmOptions, "title" | "description" | "confirmText">>
    | null;
}

interface ConfirmationDialogProps {
  isOpen: boolean;
  options: ConfirmOptions | null;
  isSubmitting: boolean;
  isPreparing: boolean;
  prepareFailed: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * Standalone dialog component - stable reference, won't cause parent re-renders
 */
function ConfirmationDialogComponent({
  isOpen,
  options,
  isSubmitting,
  isPreparing,
  prepareFailed,
  onConfirm,
  onCancel,
}: ConfirmationDialogProps) {
  if (!options) return null;

  return (
    <AlertDialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open && !isSubmitting) {
          onCancel();
        }
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{options.title}</AlertDialogTitle>
          <AlertDialogDescription>{options.description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel onClick={onCancel} disabled={isSubmitting}>
            {options.cancelText ?? "Cancel"}
          </AlertDialogCancel>
          <AlertDialogAction
            onClick={(event) => {
              event.preventDefault();
              onConfirm();
            }}
            variant={options.variant ?? "default"}
            disabled={isSubmitting || isPreparing || prepareFailed}
            className="gap-2"
          >
            {(isSubmitting || isPreparing) && (
              <Loader2 aria-hidden="true" className="h-4 w-4 animate-spin" />
            )}
            {isPreparing
              ? "Preparing..."
              : isSubmitting
              ? options.pendingText ?? "Working..."
              : options.confirmText ?? "Confirm"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

interface UseConfirmationReturn {
  confirm: (options: ConfirmOptions) => Promise<boolean>;
  confirmationDialogProps: ConfirmationDialogProps;
  ConfirmationDialog: typeof ConfirmationDialogComponent;
}

/**
 * Hook for showing confirmation dialogs with async/await pattern.
 *
 * Usage:
 * ```tsx
 * const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
 *
 * // In your component:
 * <ConfirmationDialog {...confirmationDialogProps} />
 *
 * // To show dialog:
 * const confirmed = await confirm({ title: "Delete?", description: "..." });
 * ```
 */
export function useConfirmation(): UseConfirmationReturn {
  const [isOpen, setIsOpen] = useState(false);
  const [options, setOptions] = useState<ConfirmOptions | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [isPreparing, setIsPreparing] = useState(false);
  const [prepareFailed, setPrepareFailed] = useState(false);
  const resolveRef = useRef<((value: boolean) => void) | null>(null);
  const requestIdRef = useRef(0);

  const confirm = useCallback((opts: ConfirmOptions): Promise<boolean> => {
    setOptions(opts);
    setIsSubmitting(false);
    setIsPreparing(Boolean(opts.prepare));
    setPrepareFailed(false);
    setIsOpen(true);
    const requestId = ++requestIdRef.current;
    if (opts.prepare) {
      void Promise.resolve()
        .then(opts.prepare)
        .then((prepared) => {
          if (requestId !== requestIdRef.current) return;
          if (prepared) {
            setOptions((current) => (current ? { ...current, ...prepared } : current));
          }
          setIsPreparing(false);
          setPrepareFailed(false);
        })
        .catch(() => {
          if (requestId !== requestIdRef.current) return;
          setOptions((current) =>
            current
              ? {
                  ...current,
                  description:
                    "Could not prepare this action. Cancel and try again.",
                }
              : current,
          );
          setIsPreparing(false);
          setPrepareFailed(true);
        });
    }
    return new Promise((resolve) => {
      resolveRef.current = resolve;
    });
  }, []);

  const settle = useCallback((value: boolean) => {
    requestIdRef.current += 1;
    setIsOpen(false);
    setOptions(null);
    setIsPreparing(false);
    setPrepareFailed(false);
    resolveRef.current?.(value);
    resolveRef.current = null;
  }, []);

  const onConfirm = useCallback(() => {
    if (isSubmitting || isPreparing || prepareFailed) {
      return;
    }

    const action = options?.onConfirm;
    const recoverFromError = options?.recoverFromError;
    if (!action) {
      settle(true);
      return;
    }

    const requestId = requestIdRef.current;
    setIsSubmitting(true);
    void Promise.resolve()
      .then(action)
      .then(() => {
        settle(true);
      })
      .catch((error: unknown) =>
        Promise.resolve(recoverFromError?.(error))
          .catch(() => null)
          .then((recovery) => {
            if (requestId !== requestIdRef.current) return;
            if (recovery) {
              setOptions((current) =>
                current ? { ...current, ...recovery } : current,
              );
              return;
            }
            settle(false);
          }),
      )
      .finally(() => {
        setIsSubmitting(false);
      });
  }, [isPreparing, isSubmitting, options, prepareFailed, settle]);

  const onCancel = useCallback(() => {
    if (isSubmitting) {
      return;
    }
    settle(false);
  }, [isSubmitting, settle]);

  const confirmationDialogProps = useMemo(
    () => ({
      isOpen,
      options,
      isSubmitting,
      isPreparing,
      prepareFailed,
      onConfirm,
      onCancel,
    }),
    [
      isOpen,
      options,
      isSubmitting,
      isPreparing,
      prepareFailed,
      onConfirm,
      onCancel,
    ]
  );

  return {
    confirm,
    confirmationDialogProps,
    ConfirmationDialog: ConfirmationDialogComponent,
  };
}
