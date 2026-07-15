import { forwardRef, useRef, type ReactNode, type RefObject } from "react";

import {
  AlertCircle,
  Check,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  Cpu,
  Gauge,
  Settings,
  Zap,
} from "lucide-react";

import {
  Popover,
  PopoverAnchor,
  PopoverContent,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";
import type { AgentProvider } from "@/stores/agentSessionStore";

import { ComposerRuntimeOptionList } from "./ComposerRuntimeOptionList";
import type {
  ComposerRuntimeEffortField,
  ComposerRuntimeModelField,
  ComposerRuntimeProviderField,
} from "./runtimeSelectorTypes";

export type ComposerRuntimeAdvancedLevel =
  | "overview"
  | "provider"
  | "models"
  | "effort"
  | "speed";

interface ComposerRuntimeAdvancedMenuProps {
  provider: ComposerRuntimeProviderField;
  model: ComposerRuntimeModelField;
  effort: ComposerRuntimeEffortField;
  viewingProvider: AgentProvider;
  onViewingProviderChange: (provider: AgentProvider) => void;
  level: ComposerRuntimeAdvancedLevel;
  onLevelChange: (level: ComposerRuntimeAdvancedLevel) => void;
  narrow: boolean;
  side: "left" | "right";
  nestedContentRef: RefObject<HTMLDivElement | null>;
  onBackToQuick: () => void;
}

function RuntimeLevelShell({
  testId,
  title,
  subtitle,
  narrow,
  onBack,
  children,
}: {
  testId: string;
  title: string;
  subtitle: string;
  narrow: boolean;
  onBack: () => void;
  children: ReactNode;
}) {
  return (
    <div
      data-testid={testId}
      className={cn(
        "flex max-h-[min(31rem,var(--radix-popover-content-available-height))] min-h-0 flex-col overflow-hidden rounded-xl border shadow-lg",
        narrow
          ? "w-[min(22rem,calc(100vw-1rem))]"
          : "w-[min(20rem,calc(100vw-1rem))]",
      )}
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      <div className="flex shrink-0 items-center gap-2 px-3 pb-1.5 pt-2.5">
        {narrow && (
          <button
            type="button"
            aria-label="Back to Advanced runtime settings"
            className="flex items-center gap-1 rounded-md px-1.5 py-1 text-[0.6875rem] font-medium text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-muted)]"
            onClick={onBack}
          >
            <ChevronLeft className="h-3.5 w-3.5" />
            Advanced
          </button>
        )}
        <div className="min-w-0 flex-1">
          <div className="truncate text-[0.75rem] font-semibold text-[var(--text-primary)]">
            {title}
          </div>
          <div className="truncate text-[0.625rem] text-[var(--text-muted)]">
            {subtitle}
          </div>
        </div>
      </div>
      {children}
    </div>
  );
}

function ProviderLevel({
  provider,
  viewingProvider,
  narrow,
  onBack,
  onSelect,
}: {
  provider: ComposerRuntimeProviderField;
  viewingProvider: AgentProvider;
  narrow: boolean;
  onBack: () => void;
  onSelect: (provider: AgentProvider, disabled: boolean) => void;
}) {
  const selectedOption = provider.options.find(
    (option) => option.id === viewingProvider,
  );

  return (
    <RuntimeLevelShell
      testId="agent-composer-runtime-provider-submenu"
      title="Provider"
      subtitle="Choose runtime provider"
      narrow={narrow}
      onBack={onBack}
    >
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-1.5 pb-1.5">
        <div className="space-y-0.5 py-1">
          {provider.options.map((option) => {
            const selected = option.id === viewingProvider;
            return (
              <button
                key={option.id}
                type="button"
                data-testid={`agent-composer-runtime-provider-${option.id}`}
                aria-pressed={selected}
                aria-disabled={option.disabled ?? false}
                className={cn(
                  "flex w-full items-start justify-between gap-2 rounded-md px-2 py-1.5 text-left text-[0.75rem] transition-colors",
                  selected ? "bg-[var(--accent-muted)]" : "hover:bg-[var(--bg-hover)]",
                  option.disabled && "opacity-60",
                )}
                onClick={() => onSelect(option.id, Boolean(option.disabled))}
              >
                <span className="min-w-0 flex-1">
                  <span
                    className="block truncate font-medium"
                    style={{
                      color: selected
                        ? "var(--accent-primary)"
                        : "var(--text-primary)",
                    }}
                  >
                    {option.label}
                  </span>
                  {(option.disabledReason || option.description) && (
                    <span className="mt-0.5 block line-clamp-2 text-[0.6875rem] leading-snug text-[var(--text-muted)]">
                      {option.disabledReason ?? option.description}
                    </span>
                  )}
                </span>
                {selected && (
                  <Check className="mt-0.5 h-3.5 w-3.5 shrink-0 text-[var(--accent-primary)]" />
                )}
              </button>
            );
          })}
        </div>
        {selectedOption?.disabled && (
          <div className="flex flex-col items-center gap-2 px-3 py-3 text-center">
            <AlertCircle className="h-5 w-5 text-[var(--text-muted)]" />
            <span className="text-[0.8125rem] font-medium text-[var(--text-secondary)]">
              {selectedOption.label} is not enabled
            </span>
            <span className="text-[0.6875rem] leading-snug text-[var(--text-muted)]">
              Enable this provider in settings to use its models.
            </span>
            {provider.footerAction}
          </div>
        )}
        {!selectedOption?.disabled && provider.footerAction && (
          <div className="px-1 pt-1">{provider.footerAction}</div>
        )}
      </div>
    </RuntimeLevelShell>
  );
}

function ModelLevel({
  model,
  providerLabel,
  narrow,
  onBack,
}: {
  model: ComposerRuntimeModelField;
  providerLabel: string;
  narrow: boolean;
  onBack: () => void;
}) {
  return (
    <RuntimeLevelShell
      testId="agent-composer-runtime-model-submenu"
      title="Model"
      subtitle={`${providerLabel} runtime`}
      narrow={narrow}
      onBack={onBack}
    >
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-1.5 pb-1.5">
        <ComposerRuntimeOptionList
          label="Model"
          value={model.value}
          options={model.options}
          disabled={model.disabled ?? false}
          testId={model.testId ?? "agent-composer-runtime-model"}
          icon={Cpu}
          onValueChange={model.onValueChange}
          {...(model.allowCustomValue !== undefined && {
            allowCustomValue: model.allowCustomValue,
          })}
          {...(model.customPlaceholder !== undefined && {
            customPlaceholder: model.customPlaceholder,
          })}
        />
        {model.onOpenModelSettings && (
          <button
            type="button"
            className="mb-1 flex w-full items-center justify-end gap-1.5 rounded-md px-2 py-1.5 text-[0.6875rem] text-[var(--text-muted)] hover:bg-[var(--bg-hover)]"
            onClick={model.onOpenModelSettings}
          >
            <Settings className="h-3 w-3" />
            Manage models in Settings
          </button>
        )}
      </div>
    </RuntimeLevelShell>
  );
}

function EffortLevel({
  effort,
  modelLabel,
  narrow,
  onBack,
}: {
  effort: ComposerRuntimeEffortField;
  modelLabel: string;
  narrow: boolean;
  onBack: () => void;
}) {
  return (
    <RuntimeLevelShell
      testId="agent-composer-runtime-effort-submenu"
      title="Effort"
      subtitle={`${modelLabel} reasoning`}
      narrow={narrow}
      onBack={onBack}
    >
      {effort.options.length > 0 ? (
        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-1.5 pb-1.5">
          <ComposerRuntimeOptionList
            label="Effort"
            value={effort.value}
            options={effort.options}
            disabled={effort.disabled ?? false}
            testId={effort.testId ?? "agent-composer-runtime-effort"}
            icon={Gauge}
            onValueChange={effort.onValueChange}
          />
        </div>
      ) : (
        <div className="px-3 pb-3 pt-1 text-[0.6875rem] text-[var(--text-muted)]">
          No effort options for this model
        </div>
      )}
    </RuntimeLevelShell>
  );
}

function SpeedLevel({
  model,
  narrow,
  onBack,
}: {
  model: ComposerRuntimeModelField;
  narrow: boolean;
  onBack: () => void;
}) {
  const fastMode = model.fastMode;
  if (!fastMode?.visible) return null;

  return (
    <RuntimeLevelShell
      testId="agent-composer-runtime-speed-submenu"
      title="Speed"
      subtitle="Response processing speed"
      narrow={narrow}
      onBack={onBack}
    >
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-1.5 pb-1.5">
        <ComposerRuntimeOptionList
          label="Speed"
          value={fastMode.value ? "fast" : "standard"}
          options={[
            {
              id: "standard",
              label: "Standard",
              description: "Default speed.",
            },
            {
              id: "fast",
              label: "Fast",
              description:
                fastMode.description ??
                "Use priority processing when supported.",
              ...(fastMode.disabled && {
                disabled: true,
                disabledReason:
                  fastMode.description ?? "Fast mode is unavailable.",
              }),
            },
          ]}
          disabled={false}
          testId="agent-composer-runtime-speed"
          icon={Zap}
          onValueChange={(value) => fastMode.onValueChange(value === "fast")}
        />
      </div>
    </RuntimeLevelShell>
  );
}

interface SubmenuRowProps {
  label: string;
  value: string;
  testId: string;
  expanded: boolean;
  onOpen: () => void;
  hoverToOpen: boolean;
}

const SubmenuRow = forwardRef<HTMLButtonElement, SubmenuRowProps>(
  function SubmenuRow(
    { label, value, testId, expanded, onOpen, hoverToOpen },
    ref,
  ) {
    return (
      <button
        ref={ref}
        type="button"
        data-testid={testId}
        aria-label={`${label}, ${value}`}
        aria-haspopup="dialog"
        aria-expanded={expanded}
        className={cn(
          "flex w-full items-center gap-3 rounded-md px-2.5 py-2 text-left outline-none transition-colors hover:bg-[var(--bg-hover)] focus-visible:ring-2 focus-visible:ring-[var(--accent-muted)]",
          expanded && "bg-[var(--bg-hover)]",
        )}
        onPointerMove={() => {
          if (hoverToOpen) onOpen();
        }}
        onClick={onOpen}
        onKeyDown={(event) => {
          if (event.key === "ArrowRight") {
            event.preventDefault();
            onOpen();
          }
        }}
      >
        <span className="min-w-0 flex-1 truncate text-[0.75rem] font-medium text-[var(--text-primary)]">
          {label}
        </span>
        <span className="max-w-[9rem] truncate text-[0.6875rem] text-[var(--text-muted)]">
          {value}
        </span>
        <ChevronRight className="h-3.5 w-3.5 shrink-0 text-[var(--text-muted)]" />
      </button>
    );
  },
);

function WideSubmenu({
  active,
  label,
  value,
  testId,
  side,
  triggerRef,
  contentRef,
  focusSelector,
  onOpen,
  onClose,
  children,
}: {
  active: boolean;
  label: string;
  value: string;
  testId: string;
  side: "left" | "right";
  triggerRef: RefObject<HTMLButtonElement | null>;
  contentRef: RefObject<HTMLDivElement | null>;
  focusSelector: string;
  onOpen: () => void;
  onClose: () => void;
  children: ReactNode;
}) {
  const closeAndFocus = () => {
    onClose();
    window.requestAnimationFrame(() => triggerRef.current?.focus());
  };

  return (
    <Popover
      open={active}
      onOpenChange={(nextOpen) => (nextOpen ? onOpen() : onClose())}
    >
      <PopoverAnchor asChild>
        <SubmenuRow
          ref={triggerRef}
          label={label}
          value={value}
          testId={testId}
          expanded={active}
          onOpen={onOpen}
          hoverToOpen
        />
      </PopoverAnchor>
      <PopoverContent
        ref={contentRef}
        side={side}
        align="start"
        sideOffset={8}
        collisionPadding={8}
        className="max-h-[var(--radix-popover-content-available-height)] w-auto overflow-y-auto overscroll-contain border-0 bg-transparent p-0 shadow-none"
        style={{ backgroundColor: "transparent" }}
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          window.requestAnimationFrame(() => {
            contentRef.current
              ?.querySelector<HTMLElement>(focusSelector)
              ?.focus();
          });
        }}
        onEscapeKeyDown={(event) => {
          event.preventDefault();
          closeAndFocus();
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowLeft") {
            event.preventDefault();
            closeAndFocus();
          }
        }}
      >
        {children}
      </PopoverContent>
    </Popover>
  );
}

function AdvancedMenuRow({
  narrow,
  active,
  level,
  label,
  value,
  testId,
  side,
  triggerRef,
  contentRef,
  focusSelector,
  onLevelChange,
  children,
}: {
  narrow: boolean;
  active: boolean;
  level: Exclude<ComposerRuntimeAdvancedLevel, "overview">;
  label: string;
  value: string;
  testId: string;
  side: "left" | "right";
  triggerRef: RefObject<HTMLButtonElement | null>;
  contentRef: RefObject<HTMLDivElement | null>;
  focusSelector: string;
  onLevelChange: (level: ComposerRuntimeAdvancedLevel) => void;
  children: ReactNode;
}) {
  if (narrow) {
    return (
      <SubmenuRow
        ref={triggerRef}
        label={label}
        value={value}
        testId={testId}
        expanded={false}
        onOpen={() => onLevelChange(level)}
        hoverToOpen={false}
      />
    );
  }

  return (
    <WideSubmenu
      active={active}
      label={label}
      value={value}
      testId={testId}
      side={side}
      triggerRef={triggerRef}
      contentRef={contentRef}
      focusSelector={focusSelector}
      onOpen={() => onLevelChange(level)}
      onClose={() => onLevelChange("overview")}
    >
      {children}
    </WideSubmenu>
  );
}

export function ComposerRuntimeAdvancedMenu({
  provider,
  model,
  effort,
  viewingProvider,
  onViewingProviderChange,
  level,
  onLevelChange,
  narrow,
  side,
  nestedContentRef,
  onBackToQuick,
}: ComposerRuntimeAdvancedMenuProps) {
  const providerTriggerRef = useRef<HTMLButtonElement>(null);
  const modelTriggerRef = useRef<HTMLButtonElement>(null);
  const effortTriggerRef = useRef<HTMLButtonElement>(null);
  const speedTriggerRef = useRef<HTMLButtonElement>(null);
  const viewingOption = provider.options.find(
    (option) => option.id === viewingProvider,
  );
  const viewingProviderDisabled = Boolean(viewingOption?.disabled);
  const viewingProviderLabel = viewingOption?.label ?? viewingProvider;
  const modelLabel =
    model.options.find((option) => option.id === model.value)?.label ??
    model.value;
  const effortLabel =
    effort.options.find((option) => option.id === effort.value)?.label ??
    (effort.options.length > 0 ? effort.value : "No options");
  const speedLabel = model.fastMode?.value ? "Fast" : "Standard";

  const returnToOverview = (trigger: RefObject<HTMLButtonElement | null>) => {
    onLevelChange("overview");
    window.requestAnimationFrame(() => trigger.current?.focus());
  };

  const selectProvider = (
    nextProvider: AgentProvider,
    disabled: boolean,
  ) => {
    onViewingProviderChange(nextProvider);
    if (!disabled) provider.onValueChange(nextProvider);
  };

  const levels = {
    provider: (
      <ProviderLevel
        provider={provider}
        viewingProvider={viewingProvider}
        narrow={narrow}
        onBack={() => returnToOverview(providerTriggerRef)}
        onSelect={selectProvider}
      />
    ),
    models: (
      <ModelLevel
        model={model}
        providerLabel={viewingProviderLabel}
        narrow={narrow}
        onBack={() => returnToOverview(modelTriggerRef)}
      />
    ),
    effort: (
      <EffortLevel
        effort={effort}
        modelLabel={modelLabel}
        narrow={narrow}
        onBack={() => returnToOverview(effortTriggerRef)}
      />
    ),
    speed: (
      <SpeedLevel
        model={model}
        narrow={narrow}
        onBack={() => returnToOverview(speedTriggerRef)}
      />
    ),
  };

  if (narrow && level !== "overview") {
    return levels[level];
  }

  return (
    <div
      data-testid="agent-composer-runtime-advanced"
      data-layout={narrow ? "drill-in" : "cascade"}
      className="flex w-[min(22rem,calc(100vw-1rem))] flex-col overflow-hidden rounded-xl border shadow-lg"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      <div className="min-h-0 flex-1 space-y-0.5 overflow-y-auto overscroll-contain p-1.5">
        <AdvancedMenuRow
          narrow={narrow}
          active={level === "provider"}
          level="provider"
          label="Provider"
          value={viewingProviderLabel}
          testId="agent-composer-runtime-provider-menu-trigger"
          side={side}
          triggerRef={providerTriggerRef}
          contentRef={nestedContentRef}
          focusSelector={`[data-testid="agent-composer-runtime-provider-${viewingProvider}"]`}
          onLevelChange={onLevelChange}
        >
          {levels.provider}
        </AdvancedMenuRow>
        {!viewingProviderDisabled && (
          <AdvancedMenuRow
            narrow={narrow}
            active={level === "models"}
            level="models"
            label="Model"
            value={modelLabel}
            testId="agent-composer-runtime-model-menu-trigger"
            side={side}
            triggerRef={modelTriggerRef}
            contentRef={nestedContentRef}
            focusSelector={`[data-testid="${model.testId ?? "agent-composer-runtime-model"}-${model.value}"]`}
            onLevelChange={onLevelChange}
          >
            {levels.models}
          </AdvancedMenuRow>
        )}
        {!viewingProviderDisabled && (
          <AdvancedMenuRow
            narrow={narrow}
            active={level === "effort"}
            level="effort"
            label="Effort"
            value={effortLabel}
            testId="agent-composer-runtime-effort-menu-trigger"
            side={side}
            triggerRef={effortTriggerRef}
            contentRef={nestedContentRef}
            focusSelector={`[data-testid="${effort.testId ?? "agent-composer-runtime-effort"}-${effort.value}"]`}
            onLevelChange={onLevelChange}
          >
            {levels.effort}
          </AdvancedMenuRow>
        )}
        {!viewingProviderDisabled && model.fastMode?.visible && (
          <AdvancedMenuRow
            narrow={narrow}
            active={level === "speed"}
            level="speed"
            label="Speed"
            value={speedLabel}
            testId="agent-composer-runtime-speed-menu-trigger"
            side={side}
            triggerRef={speedTriggerRef}
            contentRef={nestedContentRef}
            focusSelector={`[data-testid="agent-composer-runtime-speed-${model.fastMode.value ? "fast" : "standard"}"]`}
            onLevelChange={onLevelChange}
          >
            {levels.speed}
          </AdvancedMenuRow>
        )}
        {viewingProviderDisabled && level !== "provider" && (
          <div className="flex flex-col items-center gap-2 px-3 py-4 text-center">
            <AlertCircle className="h-5 w-5 text-[var(--text-muted)]" />
            <span className="text-[0.8125rem] font-medium text-[var(--text-secondary)]">
              {viewingProviderLabel} is not enabled
            </span>
            <span className="text-[0.6875rem] leading-snug text-[var(--text-muted)]">
              Enable this provider in settings to use its models.
            </span>
            {provider.footerAction}
          </div>
        )}
      </div>
      <div
        className="shrink-0 border-t p-1.5"
        style={{
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px 0 0",
        }}
      >
        <button
          type="button"
          aria-label="Advanced, switch to Quick runtime settings"
          className="flex w-full items-center justify-between rounded-md px-2.5 py-2 text-[0.75rem] font-medium text-[var(--text-secondary)] outline-none hover:bg-[var(--bg-hover)] focus-visible:ring-2 focus-visible:ring-[var(--accent-muted)]"
          onClick={onBackToQuick}
        >
          <span>Advanced</span>
          <ChevronUp className="h-3.5 w-3.5" />
        </button>
      </div>
    </div>
  );
}
