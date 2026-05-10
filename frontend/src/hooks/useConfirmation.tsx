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

interface ConfirmOptions {
  title: string;
  description: string;
  confirmText?: string;
  pendingText?: string;
  cancelText?: string;
  variant?: "default" | "destructive";
  onConfirm?: () => Promise<unknown> | unknown;
}

interface ConfirmationDialogProps {
  isOpen: boolean;
  options: ConfirmOptions | null;
  isSubmitting: boolean;
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
            disabled={isSubmitting}
            className="gap-2"
          >
            {isSubmitting && (
              <Loader2 aria-hidden="true" className="h-4 w-4 animate-spin" />
            )}
            {isSubmitting
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
  const resolveRef = useRef<((value: boolean) => void) | null>(null);

  const confirm = useCallback((opts: ConfirmOptions): Promise<boolean> => {
    setOptions(opts);
    setIsSubmitting(false);
    setIsOpen(true);
    return new Promise((resolve) => {
      resolveRef.current = resolve;
    });
  }, []);

  const settle = useCallback((value: boolean) => {
    setIsOpen(false);
    setOptions(null);
    resolveRef.current?.(value);
    resolveRef.current = null;
  }, []);

  const onConfirm = useCallback(() => {
    if (isSubmitting) {
      return;
    }

    const action = options?.onConfirm;
    if (!action) {
      settle(true);
      return;
    }

    setIsSubmitting(true);
    void Promise.resolve()
      .then(action)
      .then(() => {
        settle(true);
      })
      .catch(() => {
        settle(false);
      })
      .finally(() => {
        setIsSubmitting(false);
      });
  }, [isSubmitting, options, settle]);

  const onCancel = useCallback(() => {
    if (isSubmitting) {
      return;
    }
    settle(false);
  }, [isSubmitting, settle]);

  const confirmationDialogProps = useMemo(
    () => ({ isOpen, options, isSubmitting, onConfirm, onCancel }),
    [isOpen, options, isSubmitting, onConfirm, onCancel]
  );

  return {
    confirm,
    confirmationDialogProps,
    ConfirmationDialog: ConfirmationDialogComponent,
  };
}
