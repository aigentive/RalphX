import type { TicketingProvider, TicketingProviderSummary } from "@/api/ticketing";
import { cn } from "@/lib/utils";

interface ProviderSwitcherProps {
  providers: TicketingProviderSummary[];
  activeProvider: TicketingProvider | null;
  onProviderChange: (provider: TicketingProvider) => void;
}

export function ProviderSwitcher({
  providers,
  activeProvider,
  onProviderChange,
}: ProviderSwitcherProps) {
  if (providers.length <= 1) {
    const provider = providers[0];
    return provider ? (
      <div
        className="inline-flex h-8 items-center rounded-md px-3 text-xs font-medium"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
          color: "var(--text-secondary)",
        }}
      >
        {provider.label}
      </div>
    ) : null;
  }

  return (
    <div
      className="inline-flex h-8 items-center rounded-md p-0.5"
      role="tablist"
      aria-label="Ticketing provider"
      style={{
        backgroundColor: "var(--bg-sunken)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      {providers.map((provider) => {
        const isActive = provider.provider === activeProvider;
        return (
          <button
            key={provider.provider}
            type="button"
            role="tab"
            aria-selected={isActive}
            className={cn(
              "h-7 rounded px-3 text-xs font-medium transition-colors",
              "focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]",
            )}
            style={{
              backgroundColor: isActive ? "var(--bg-elevated)" : "transparent",
              color: isActive ? "var(--text-primary)" : "var(--text-muted)",
            }}
            onClick={() => onProviderChange(provider.provider)}
          >
            {provider.label}
          </button>
        );
      })}
    </div>
  );
}
