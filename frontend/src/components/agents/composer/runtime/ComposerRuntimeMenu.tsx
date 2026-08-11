import { useRef, type RefObject } from "react";

import { AlertCircle } from "lucide-react";

import type { AgentProvider } from "@/stores/agentSessionStore";

import { ComposerRuntimeEffortScale } from "./ComposerRuntimeEffortScale";
import {
  ComposerRuntimeCapabilityLevel,
  ComposerRuntimeEffortLevel,
  ComposerRuntimeModelLevel,
  ComposerRuntimePersonaLevel,
  ComposerRuntimeProviderLevel,
  ComposerRuntimeSpeedLevel,
} from "./ComposerRuntimeMenuLevels";
import { ComposerRuntimeMenuHeader } from "./ComposerRuntimeMenuHeader";
import {
  ComposerRuntimeMenuRow,
  type ComposerRuntimeMenuLevel,
} from "./ComposerRuntimeMenuRows";
import type {
  ComposerRuntimeCapabilityField,
  ComposerRuntimeEffortField,
  ComposerRuntimeModelField,
  ComposerRuntimePersonaField,
  ComposerRuntimeProviderField,
  ComposerRuntimeSpeedField,
} from "./runtimeSelectorTypes";

interface ComposerRuntimeMenuProps {
  provider: ComposerRuntimeProviderField;
  model: ComposerRuntimeModelField;
  effort: ComposerRuntimeEffortField;
  capability?: ComposerRuntimeCapabilityField;
  persona?: ComposerRuntimePersonaField;
  speed?: ComposerRuntimeSpeedField;
  runtimeDefault?: {
    source?: string | null;
    scopeLabel?: string;
    isResetting?: boolean;
    disabled?: boolean;
    onReset: () => Promise<unknown> | void;
  };
  viewingProvider: AgentProvider;
  onPreviewProviderChange: (provider: AgentProvider | null) => void;
  level: ComposerRuntimeMenuLevel;
  onLevelChange: (level: ComposerRuntimeMenuLevel) => void;
  narrow: boolean;
  side: "left" | "right";
  nestedContentRef: RefObject<HTMLDivElement | null>;
  previewIndex: number | null;
  onPreviewChange: (index: number | null) => void;
}

export function ComposerRuntimeMenu({
  provider,
  model,
  effort,
  capability,
  persona,
  speed,
  runtimeDefault,
  viewingProvider,
  onPreviewProviderChange,
  level,
  onLevelChange,
  narrow,
  side,
  nestedContentRef,
  previewIndex,
  onPreviewChange,
}: ComposerRuntimeMenuProps) {
  const providerTriggerRef = useRef<HTMLButtonElement>(null);
  const modelTriggerRef = useRef<HTMLButtonElement>(null);
  const effortTriggerRef = useRef<HTMLButtonElement>(null);
  const capabilityTriggerRef = useRef<HTMLButtonElement>(null);
  const personaTriggerRef = useRef<HTMLButtonElement>(null);
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
  const capabilityLabel = capability
    ? (capability.options.find((option) => option.id === capability.value)
        ?.label ?? capability.value)
    : "";
  const personaLabel = persona
    ? (persona.options.find((option) => option.id === persona.value)?.label ?? persona.value)
    : "";
  const speedLabel = speed
    ? (speed.options.find((option) => option.id === speed.value)?.label ?? speed.value)
    : "";

  const returnToOverview = (trigger: RefObject<HTMLButtonElement | null>) => {
    onLevelChange("overview");
    window.requestAnimationFrame(() => trigger.current?.focus());
  };

  const selectProvider = (
    nextProvider: AgentProvider,
    disabled: boolean,
  ) => {
    if (disabled) {
      onPreviewProviderChange(nextProvider);
      return;
    }
    if (!provider.disabled) {
      onPreviewProviderChange(null);
      provider.onValueChange(nextProvider);
      returnToOverview(providerTriggerRef);
    }
  };

  const providerLevel = (
    <ComposerRuntimeProviderLevel
      provider={provider}
      viewingProvider={viewingProvider}
      narrow={narrow}
      onBack={() => returnToOverview(providerTriggerRef)}
      onSelect={selectProvider}
    />
  );
  const modelLevel = (
    <ComposerRuntimeModelLevel
      model={model}
      providerLabel={viewingProviderLabel}
      narrow={narrow}
      onBack={() => returnToOverview(modelTriggerRef)}
      onSettled={() => returnToOverview(modelTriggerRef)}
    />
  );
  const effortLevel = (
    <ComposerRuntimeEffortLevel
      effort={effort}
      modelLabel={modelLabel}
      narrow={narrow}
      onBack={() => returnToOverview(effortTriggerRef)}
      onSettled={() => returnToOverview(effortTriggerRef)}
    />
  );
  const capabilityLevel = capability ? (
    <ComposerRuntimeCapabilityLevel
      capability={capability}
      narrow={narrow}
      onBack={() => returnToOverview(capabilityTriggerRef)}
      onSettled={() => returnToOverview(capabilityTriggerRef)}
    />
  ) : null;
  const speedLevel = (
    <ComposerRuntimeSpeedLevel
      speed={speed!}
      narrow={narrow}
      onBack={() => returnToOverview(speedTriggerRef)}
      onSettled={() => returnToOverview(speedTriggerRef)}
    />
  );
  const personaLevel = persona ? (
    <ComposerRuntimePersonaLevel
      persona={persona}
      narrow={narrow}
      onBack={() => returnToOverview(personaTriggerRef)}
      onSettled={() => returnToOverview(personaTriggerRef)}
    />
  ) : null;

  if (narrow && level !== "overview") {
    if (level === "provider") return providerLevel;
    if (level === "models") return modelLevel;
    if (level === "effort") return effortLevel;
    if (level === "capability" && capabilityLevel) return capabilityLevel;
    if (level === "persona" && personaLevel) return personaLevel;
    if (level === "speed" && speed) return speedLevel;
  }

  return (
    <div
      data-testid="agent-composer-runtime-menu"
      data-layout={narrow ? "drill-in" : "cascade"}
      className="flex max-h-[min(38rem,var(--radix-popover-content-available-height))] min-h-0 w-[min(22rem,calc(100vw-1rem))] flex-col overflow-hidden rounded-xl border shadow-lg"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      <ComposerRuntimeMenuHeader
        model={model}
        {...(runtimeDefault ? { runtimeDefault } : {})}
      />
      <div
        data-testid="agent-composer-runtime-menu-scroll"
        className="min-h-0 flex-1 overflow-y-auto overscroll-contain"
      >
        <div className="space-y-0.5 p-1.5">
          <ComposerRuntimeMenuRow
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
            disabled={provider.disabled ?? false}
          >
            {providerLevel}
          </ComposerRuntimeMenuRow>
          {!viewingProviderDisabled && (
            <ComposerRuntimeMenuRow
              narrow={narrow}
              active={level === "models"}
              level="models"
              label="Model"
              value={modelLabel || "Unavailable"}
              testId="agent-composer-runtime-model-menu-trigger"
              side={side}
              triggerRef={modelTriggerRef}
              contentRef={nestedContentRef}
              focusSelector={`[data-testid="${model.testId ?? "agent-composer-runtime-model"}-${model.value}"]`}
              onLevelChange={onLevelChange}
              disabled={model.disabled ?? false}
            >
              {modelLevel}
            </ComposerRuntimeMenuRow>
          )}
          {!viewingProviderDisabled && (
            <ComposerRuntimeMenuRow
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
              disabled={effort.disabled ?? false}
            >
              {effortLevel}
            </ComposerRuntimeMenuRow>
          )}
          {!viewingProviderDisabled && capability && capabilityLevel && (
            <ComposerRuntimeMenuRow
              narrow={narrow}
              active={level === "capability"}
              level="capability"
              label="Capabilities"
              value={capabilityLabel}
              testId="agent-composer-runtime-capability-menu-trigger"
              side={side}
              triggerRef={capabilityTriggerRef}
              contentRef={nestedContentRef}
              focusSelector={`[data-testid="${capability.testId ?? "agent-composer-capability"}-${capability.value}"]`}
              onLevelChange={onLevelChange}
              disabled={Boolean(capability.disabled || capability.pending)}
              pending={capability.pending ?? false}
            >
              {capabilityLevel}
            </ComposerRuntimeMenuRow>
          )}
          {!viewingProviderDisabled && persona && personaLevel && (
            <ComposerRuntimeMenuRow
              narrow={narrow}
              active={level === "persona"}
              level="persona"
              label="Persona"
              value={personaLabel}
              testId="agent-composer-runtime-persona-menu-trigger"
              side={side}
              triggerRef={personaTriggerRef}
              contentRef={nestedContentRef}
              focusSelector={`[data-testid="${persona.testId ?? "agent-composer-runtime-persona"}-${persona.value}"]`}
              onLevelChange={onLevelChange}
              disabled={persona.disabled ?? false}
            >
              {personaLevel}
            </ComposerRuntimeMenuRow>
          )}
          {!viewingProviderDisabled && speed && (
            <ComposerRuntimeMenuRow
              narrow={narrow}
              active={level === "speed"}
              level="speed"
              label="Speed"
              value={speedLabel}
              testId="agent-composer-runtime-speed-menu-trigger"
              side={side}
              triggerRef={speedTriggerRef}
              contentRef={nestedContentRef}
              focusSelector={`[data-testid="agent-composer-runtime-speed-${speed.value}"]`}
              onLevelChange={onLevelChange}
            >
              {speedLevel}
            </ComposerRuntimeMenuRow>
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
        {!viewingProviderDisabled && effort.options.length > 0 && (
          <div
            className="border-t"
            style={{
              borderColor: "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: "1px 0 0",
            }}
          >
            <ComposerRuntimeEffortScale
              value={effort.value}
              options={effort.options}
              previewIndex={previewIndex}
              onPreviewChange={onPreviewChange}
              onCommit={effort.onValueChange}
              disabled={effort.disabled ?? false}
            />
          </div>
        )}
      </div>
    </div>
  );
}
