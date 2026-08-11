import { useState, type ReactNode } from "react";

import {
  AlertCircle,
  Check,
  ChevronLeft,
  Cpu,
  Gauge,
  Layers3,
  Settings,
  UserRound,
  Zap,
} from "lucide-react";

import { cn } from "@/lib/utils";
import type { AgentProvider } from "@/stores/agentSessionStore";

import { ComposerRuntimeOptionList } from "./ComposerRuntimeOptionList";
import type {
  ComposerRuntimeCapabilityField,
  ComposerRuntimeEffortField,
  ComposerRuntimeModelField,
  ComposerRuntimePersonaField,
  ComposerRuntimeProviderField,
  ComposerRuntimeSpeedField,
} from "./runtimeSelectorTypes";

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

export function ComposerRuntimeProviderLevel({
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

export function ComposerRuntimeModelLevel({
  model,
  providerLabel,
  narrow,
  onBack,
  onSettled,
}: {
  model: ComposerRuntimeModelField;
  providerLabel: string;
  narrow: boolean;
  onBack: () => void;
  onSettled: () => void;
}) {
  const selectModel = (value: string) => {
    model.onValueChange(value);
    onSettled();
  };

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
          onValueChange={selectModel}
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

export function ComposerRuntimeEffortLevel({
  effort,
  modelLabel,
  narrow,
  onBack,
  onSettled,
}: {
  effort: ComposerRuntimeEffortField;
  modelLabel: string;
  narrow: boolean;
  onBack: () => void;
  onSettled: () => void;
}) {
  const selectEffort = (value: string) => {
    effort.onValueChange(value);
    onSettled();
  };

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
            onValueChange={selectEffort}
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

export function ComposerRuntimeCapabilityLevel({
  capability,
  narrow,
  onBack,
  onSettled,
}: {
  capability: ComposerRuntimeCapabilityField;
  narrow: boolean;
  onBack: () => void;
  onSettled: () => void;
}) {
  const [settling, setSettling] = useState(false);
  const testId = capability.testId ?? "agent-composer-capability";
  const disabled = Boolean(
    capability.disabled || capability.pending || settling,
  );

  const selectCapability = async (value: string) => {
    if (disabled) return;
    setSettling(true);
    try {
      await capability.onValueChange(
        value as ComposerRuntimeCapabilityField["value"],
      );
      onSettled();
    } catch {
      // The parent owns error reporting and the controlled selection value.
    } finally {
      setSettling(false);
    }
  };

  return (
    <RuntimeLevelShell
      testId="agent-composer-runtime-capability-submenu"
      title="Capabilities"
      subtitle="Agent coordination"
      narrow={narrow}
      onBack={onBack}
    >
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-1.5 pb-1.5">
        <ComposerRuntimeOptionList
          label="Capabilities"
          value={capability.value}
          options={capability.options}
          disabled={disabled}
          testId={testId}
          icon={Layers3}
          onValueChange={(value) => void selectCapability(value)}
        />
      </div>
    </RuntimeLevelShell>
  );
}

export function ComposerRuntimeSpeedLevel({
  speed,
  narrow,
  onBack,
  onSettled,
}: {
  speed: ComposerRuntimeSpeedField;
  narrow: boolean;
  onBack: () => void;
  onSettled: () => void;
}) {
  const selectSpeed = (value: string) => {
    speed.onValueChange(value);
    onSettled();
  };

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
          value={speed.value}
          options={speed.options}
          disabled={speed.disabled ?? false}
          testId={speed.testId ?? "agent-composer-runtime-speed"}
          icon={Zap}
          onValueChange={selectSpeed}
        />
      </div>
    </RuntimeLevelShell>
  );
}

export function ComposerRuntimePersonaLevel({
  persona,
  narrow,
  onBack,
  onSettled,
}: {
  persona: ComposerRuntimePersonaField;
  narrow: boolean;
  onBack: () => void;
  onSettled: () => void;
}) {
  const [settling, setSettling] = useState(false);
  const disabled = Boolean(persona.disabled || settling);

  const selectPersona = async (value: string) => {
    if (disabled) return;
    setSettling(true);
    try {
      await persona.onValueChange(value);
      onSettled();
    } catch {
      // The parent owns error reporting and the controlled selection value.
    } finally {
      setSettling(false);
    }
  };

  return (
    <RuntimeLevelShell
      testId="agent-composer-runtime-persona-submenu"
      title="Persona"
      subtitle="Voice and working style"
      narrow={narrow}
      onBack={onBack}
    >
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-1.5 pb-1.5">
        <ComposerRuntimeOptionList
          label="Persona"
          value={persona.value}
          options={persona.options}
          disabled={disabled}
          testId={persona.testId ?? "agent-composer-runtime-persona"}
          icon={UserRound}
          onValueChange={(value) => void selectPersona(value)}
        />
        {persona.footerAction}
      </div>
    </RuntimeLevelShell>
  );
}
