import { Settings } from "lucide-react";

import { Button } from "@/components/ui/button";

export function AgentProviderSettingsButton({
  onClick,
  testId,
}: {
  onClick: () => void;
  testId?: string;
}) {
  return (
    <Button
      type="button"
      variant="ghost"
      className="h-8 w-full justify-start rounded-md px-2 text-[0.75rem]"
      style={{ color: "var(--text-secondary)" }}
      onClick={onClick}
      data-testid={testId}
    >
      <Settings className="h-3.5 w-3.5" />
      Open Provider Settings
    </Button>
  );
}
