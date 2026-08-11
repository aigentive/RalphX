/* eslint-disable react-refresh/only-export-components */
import { useState, useRef, useCallback, useMemo, type ReactNode } from "react";
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
  body?: ReactNode;
  /** Keep body controls visible but immutable after a partial backend commit. */
  bodyDisabled?: boolean;
  confirmDisabled?: boolean;
  /** Close once the user has committed intent; the action continues outside the dialog. */
  closeOnConfirm?: boolean;
  /** Starts progress UI immediately when `closeOnConfirm` captures intent. */
  onIntent?: () => void;
  /** Reports a detached action failure after its dialog has closed. */
  onErrorAfterClose?: (error: unknown) => void;
  onConfirm?: () => Promise<unknown> | unknown;
  /** Runs after the dialog shell opens; return copy updates for the same dialog. */
  prepare?: (controller: ConfirmationController) =>
    | Promise<ConfirmOptionUpdate>
    | ConfirmOptionUpdate;
  /** Map a preparation failure to caller-specific copy and disabled state. */
  recoverFromPrepareError?: (
    error: unknown,
  ) => Promise<ConfirmOptionUpdate | null> | ConfirmOptionUpdate | null;
  /** Return updated copy after a recoverable action error to keep the dialog open for reconfirmation. */
  recoverFromError?: (
    error: unknown,
  ) =>
    | Promise<
        ConfirmOptionUpdate | null
      >
    | ConfirmOptionUpdate
    | null;
}

export interface ConfirmationController {
  update: (patch: Partial<ConfirmOptions>) => boolean;
  isCurrent: () => boolean;
}
export type ConfirmOptionUpdate = Partial<
  Pick<
    ConfirmOptions,
    | "title"
    | "description"
    | "confirmText"
    | "confirmDisabled"
    | "body"
    | "bodyDisabled"
  >
>;

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
          {options.body && (
            <fieldset
              disabled={isSubmitting || options.bodyDisabled === true}
              className="m-0 min-w-0 border-0 p-0"
            >
              {options.body}
            </fieldset>
          )}
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
            disabled={
              isSubmitting ||
              isPreparing ||
              prepareFailed ||
              options.confirmDisabled === true
            }
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
  const resolveRef = useRef<{
    requestId: number;
    resolve: (value: boolean) => void;
  } | null>(null);
  const requestIdRef = useRef(0);

  const confirm = useCallback((opts: ConfirmOptions): Promise<boolean> => {
    const requestId = ++requestIdRef.current;
    resolveRef.current?.resolve(false);
    setOptions(opts);
    setIsSubmitting(false);
    setIsPreparing(Boolean(opts.prepare));
    setPrepareFailed(false);
    setIsOpen(true);
    if (opts.prepare) {
      const controller: ConfirmationController = {
        update: (patch) => {
          if (requestId !== requestIdRef.current) return false;
          setOptions((current) => (current ? { ...current, ...patch } : current));
          return true;
        },
        isCurrent: () => requestId === requestIdRef.current,
      };
      const prepareAfterPaint = () =>
        new Promise<void>((resolve) => {
          window.requestAnimationFrame(() => window.setTimeout(resolve, 0));
        });
      void prepareAfterPaint()
        .then(() => opts.prepare?.(controller))
        .then((prepared) => {
          if (requestId !== requestIdRef.current) return;
          if (prepared) {
            setOptions((current) => (current ? { ...current, ...prepared } : current));
          }
          setIsPreparing(false);
          setPrepareFailed(false);
        })
        .catch((error: unknown) => {
          if (requestId !== requestIdRef.current) return;
          void Promise.resolve()
            .then(() => opts.recoverFromPrepareError?.(error) ?? null)
            .catch(() => null)
            .then((recovery) => {
              if (requestId !== requestIdRef.current) return;
              setOptions((current) => {
                if (!current) return current;
                if (recovery) return { ...current, ...recovery };
                return {
                  ...current,
                  description:
                    "Could not prepare this action. Cancel and try again.",
                };
              });
              setIsPreparing(false);
              setPrepareFailed(!recovery);
            });
        });
    }
    return new Promise((resolve) => {
      resolveRef.current = { requestId, resolve };
    });
  }, []);

  const settle = useCallback((requestId: number, value: boolean) => {
    if (requestId !== requestIdRef.current) return false;
    const pending = resolveRef.current;
    if (!pending || pending.requestId !== requestId) return false;
    requestIdRef.current += 1;
    setIsOpen(false);
    setOptions(null);
    setIsPreparing(false);
    setPrepareFailed(false);
    resolveRef.current = null;
    pending.resolve(value);
    return true;
  }, []);

  const onConfirm = useCallback(() => {
    if (
      isSubmitting ||
      isPreparing ||
      prepareFailed ||
      options?.confirmDisabled === true
    ) {
      return;
    }

    const requestId = requestIdRef.current;
    const action = options?.onConfirm;
    const recoverFromError = options?.recoverFromError;
    if (!action) {
      settle(requestId, true);
      return;
    }

    if (options.closeOnConfirm) {
      options.onIntent?.();
      settle(requestId, true);
      void Promise.resolve()
        .then(action)
        .catch((error: unknown) => {
          options.onErrorAfterClose?.(error);
        });
      return;
    }

    setIsSubmitting(true);
    void Promise.resolve()
      .then(action)
      .then(() => {
        settle(requestId, true);
      })
      .catch((error: unknown) =>
        Promise.resolve()
          .then(() => recoverFromError?.(error) ?? null)
          .catch(() => null)
          .then((recovery) => {
            if (requestId !== requestIdRef.current) return;
            if (recovery) {
              setOptions((current) =>
                current ? { ...current, ...recovery } : current,
              );
              return;
            }
            settle(requestId, false);
          }),
      )
      .finally(() => {
        if (requestId !== requestIdRef.current) return;
        setIsSubmitting(false);
      });
  }, [isPreparing, isSubmitting, options, prepareFailed, settle]);

  const onCancel = useCallback(() => {
    if (isSubmitting) {
      return;
    }
    settle(requestIdRef.current, false);
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
