import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

interface ReviewWalkthroughNavigationButtonProps {
  testId: string;
  label: string;
  children: string;
  disabled?: boolean;
  onClick: () => void;
}

export function ReviewWalkthroughNavigationButton({
  testId,
  label,
  children,
  disabled,
  onClick,
}: ReviewWalkthroughNavigationButtonProps) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          data-testid={testId}
          aria-label={label}
          disabled={disabled}
          onClick={onClick}
          className="flex h-7 w-7 items-center justify-center rounded text-xs transition-colors hover:bg-[var(--bg-hover)] disabled:cursor-not-allowed disabled:opacity-40"
          style={{
            backgroundColor: "var(--bg-elevated)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-secondary)",
          }}
        >
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent side="top">
        <p>{label}</p>
      </TooltipContent>
    </Tooltip>
  );
}
