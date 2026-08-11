import { Copy } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

export interface CopyableRefProps {
  value: string;
  prefixLabel?: string;
  ariaLabel?: string;
  testId?: string;
}

function copySubject(ariaLabel: string): string {
  const subject = ariaLabel.replace(/^copy\s+/i, "").trim() || "reference";
  return subject;
}

export function CopyableRef({
  value,
  prefixLabel,
  ariaLabel = "Copy reference",
  testId,
}: CopyableRefProps) {
  const subject = copySubject(ariaLabel);
  const copyValue = async () => {
    try {
      if (!navigator.clipboard) {
        throw new Error("clipboard unavailable");
      }
      await navigator.clipboard.writeText(value);
      toast.success(`${subject.charAt(0).toUpperCase()}${subject.slice(1)} copied`);
    } catch {
      toast.error(`Failed to copy ${subject.toLowerCase()}`);
    }
  };

  return (
    <span
      className="inline-flex min-w-0 max-w-full items-center gap-1.5 align-middle"
      {...(testId ? { "data-testid": testId } : {})}
    >
      {prefixLabel ? (
        <span className="shrink-0" style={{ color: "var(--text-muted, #8e8e96)" }}>
          {prefixLabel}
        </span>
      ) : null}
      <code
        className="min-w-0 truncate font-mono text-[0.8125rem]"
        {...(testId ? { "data-testid": `${testId}-value` } : {})}
      >
        {value}
      </code>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label={ariaLabel}
            className="h-6 w-6 shrink-0 text-[var(--text-muted)] hover:text-[var(--text-primary)]"
            onClick={() => void copyValue()}
            {...(testId ? { "data-testid": `${testId}-copy` } : {})}
          >
            <Copy className="h-3.5 w-3.5" aria-hidden="true" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>{ariaLabel}</TooltipContent>
      </Tooltip>
    </span>
  );
}
