import { Paperclip } from "lucide-react";

export interface ChatAttachmentDropOverlayProps {
  message?: string;
  roundedClassName?: string;
}

export function ChatAttachmentDropOverlay({
  message = "Drop files to attach",
  roundedClassName = "rounded-lg",
}: ChatAttachmentDropOverlayProps) {
  return (
    <div
      data-testid="chat-composer-drop-overlay"
      className={`pointer-events-none absolute inset-0 z-20 flex items-center justify-center ${roundedClassName}`}
      style={{
        backgroundColor: "color-mix(in srgb, var(--accent-primary) 12%, var(--bg-surface) 88%)",
        borderColor: "var(--accent-primary)",
        borderStyle: "dashed",
        borderWidth: "2px",
        color: "var(--accent-primary)",
      }}
    >
      <div
        className="flex items-center gap-2 rounded-full px-3 py-1.5 text-[0.8125rem] font-medium"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--accent-primary)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <Paperclip className="h-4 w-4" />
        <span>{message}</span>
      </div>
    </div>
  );
}
