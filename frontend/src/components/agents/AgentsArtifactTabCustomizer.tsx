import { Check, Plus } from "lucide-react";
import type { ElementType } from "react";

import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type { AgentArtifactTab } from "@/stores/agentSessionStore";

export interface AgentArtifactTabCustomizerItem {
  id: AgentArtifactTab;
  label: string;
  icon: ElementType;
  available: boolean;
  unavailableReason?: string;
}

interface AgentsArtifactTabCustomizerProps {
  tabs: readonly AgentArtifactTabCustomizerItem[];
  hiddenTabs: readonly AgentArtifactTab[];
  onHide: (tab: AgentArtifactTab) => void;
  onShow: (tab: AgentArtifactTab) => void;
  triggerVariant?: "icon" | "button";
}

function TabToggleRow({
  item,
  shown,
  onToggle,
}: {
  item: AgentArtifactTabCustomizerItem;
  shown: boolean;
  onToggle: () => void;
}) {
  const Icon = item.icon;
  return (
    <button
      type="button"
      aria-label={`${shown ? "Hide" : "Show"} ${item.label}`}
      aria-pressed={shown}
      onClick={onToggle}
      className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm outline-none transition-colors hover:bg-[var(--overlay-subtle)] focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
      style={{ color: "var(--text-primary)" }}
    >
      <span
        aria-hidden="true"
        className="flex size-4 shrink-0 items-center justify-center rounded border"
        style={{
          backgroundColor: shown ? "var(--accent-primary)" : "transparent",
          borderColor: shown ? "var(--accent-primary)" : "var(--overlay-medium)",
          borderWidth: 1,
          borderStyle: "solid",
          color: shown ? "var(--text-on-accent)" : "transparent",
        }}
      >
        <Check className="size-3" />
      </span>
      <Icon className="size-4 shrink-0" style={{ color: "var(--text-muted)" }} />
      <span className="truncate">{item.label}</span>
    </button>
  );
}

function UnavailableTabRow({ item }: { item: AgentArtifactTabCustomizerItem }) {
  const Icon = item.icon;
  return (
    <div className="flex gap-2 px-2 py-1.5 opacity-70">
      <Icon className="mt-0.5 size-4 shrink-0" style={{ color: "var(--text-muted)" }} />
      <div className="min-w-0">
        <div className="text-sm" style={{ color: "var(--text-secondary)" }}>
          {item.label}
        </div>
        <div className="text-xs leading-snug" style={{ color: "var(--text-muted)" }}>
          {item.unavailableReason}
        </div>
      </div>
    </div>
  );
}

export function AgentsArtifactTabCustomizer({
  tabs,
  hiddenTabs,
  onHide,
  onShow,
  triggerVariant = "icon",
}: AgentsArtifactTabCustomizerProps) {
  const hidden = new Set(hiddenTabs);
  const shownTabs = tabs.filter((tab) => tab.available && !hidden.has(tab.id));
  const userHiddenTabs = tabs.filter((tab) => tab.available && hidden.has(tab.id));
  const unavailableTabs = tabs.filter((tab) => !tab.available);

  const trigger = (
    <Button
      type="button"
      variant={triggerVariant === "icon" ? "ghost" : "outline"}
      size={triggerVariant === "icon" ? "icon" : "sm"}
      aria-label="Customize tabs"
      className={cn(
        triggerVariant === "icon"
          ? "size-8 shrink-0 text-[var(--text-muted)]"
          : "gap-2",
      )}
    >
      <Plus className="size-4" />
      {triggerVariant === "button" ? <span>Customize tabs</span> : null}
    </Button>
  );

  return (
    <Popover>
      {triggerVariant === "icon" ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <PopoverTrigger asChild>{trigger}</PopoverTrigger>
          </TooltipTrigger>
          <TooltipContent side="bottom">Customize tabs</TooltipContent>
        </Tooltip>
      ) : (
        <PopoverTrigger asChild>{trigger}</PopoverTrigger>
      )}
      <PopoverContent
        role="dialog"
        aria-label="Customize artifact tabs"
        align="end"
        sideOffset={8}
        className="w-80 max-h-[min(520px,var(--radix-popover-content-available-height))] overflow-y-auto p-3"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--overlay-medium)",
          borderWidth: 1,
          borderStyle: "solid",
        }}
      >
        <div className="px-2 pb-2 text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
          Tabs
        </div>

        {shownTabs.length > 0 ? (
          <section aria-labelledby="artifact-tabs-shown-heading" className="mb-3">
            <h3
              id="artifact-tabs-shown-heading"
              className="px-2 pb-1 text-[0.65rem] font-semibold uppercase tracking-[0.12em]"
              style={{ color: "var(--text-muted)" }}
            >
              Shown
            </h3>
            {shownTabs.map((item) => (
              <TabToggleRow
                key={item.id}
                item={item}
                shown
                onToggle={() => onHide(item.id)}
              />
            ))}
          </section>
        ) : null}

        {userHiddenTabs.length > 0 ? (
          <section aria-labelledby="artifact-tabs-hidden-heading" className="mb-3">
            <h3
              id="artifact-tabs-hidden-heading"
              className="px-2 pb-1 text-[0.65rem] font-semibold uppercase tracking-[0.12em]"
              style={{ color: "var(--text-muted)" }}
            >
              Hidden
            </h3>
            {userHiddenTabs.map((item) => (
              <TabToggleRow
                key={item.id}
                item={item}
                shown={false}
                onToggle={() => onShow(item.id)}
              />
            ))}
          </section>
        ) : null}

        {unavailableTabs.length > 0 ? (
          <section
            aria-labelledby="artifact-tabs-unavailable-heading"
            className="border-t pt-3"
            style={{
              borderColor: "var(--overlay-faint)",
              borderWidth: 1,
              borderStyle: "solid",
              borderLeftWidth: 0,
              borderRightWidth: 0,
              borderBottomWidth: 0,
            }}
          >
            <h3
              id="artifact-tabs-unavailable-heading"
              className="px-2 pb-1 text-[0.65rem] font-semibold uppercase tracking-[0.12em]"
              style={{ color: "var(--text-muted)" }}
            >
              Not available in this conversation
            </h3>
            {unavailableTabs.map((item) => (
              <UnavailableTabRow key={item.id} item={item} />
            ))}
          </section>
        ) : null}

        <p className="px-2 pt-3 text-xs" style={{ color: "var(--text-muted)" }}>
          Applies to this conversation.
        </p>
      </PopoverContent>
    </Popover>
  );
}
