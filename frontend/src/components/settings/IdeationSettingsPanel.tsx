/**
 * Shared Tasks and Planning gate configuration
 *
 * Features:
 * - Model-native verification policy and acceptance gate controls
 * - Finalization gate (requireAcceptForFinalize)
 * - Auto-accept finalization convenience toggle (in-memory only)
 * - Collapsible External Session Overrides subsection (3-state inherit/on/off selects)
 * - Follows SettingsView pattern with SettingRow and shadcn components
 */

import { useState } from "react";
import { ShieldCheck, ChevronDown, ChevronRight } from "lucide-react";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { useIdeationSettings } from "@/hooks/useIdeationSettings";
import { useUiStore } from "@/stores/uiStore";
import type { ExternalIdeationOverrides } from "@/types/ideation-config";
import { SectionCard } from "./SettingsView.shared";

// ============================================================================
// Setting Row Component
// ============================================================================

interface SettingRowProps {
  id: string;
  label: string;
  description: string;
  children: React.ReactNode;
  isSubSetting?: boolean;
  isDisabled?: boolean;
}

function SettingRow({
  id,
  label,
  description,
  children,
  isSubSetting = false,
  isDisabled = false,
}: SettingRowProps) {
  return (
    <div
      className={cn(
        "flex items-start justify-between py-3 border-b border-[var(--border-subtle)] last:border-0 -mx-2 px-2 rounded-md transition-colors",
        !isDisabled && "hover:bg-[var(--bg-hover)]",
        isDisabled && "opacity-50"
      )}
    >
      <div
        className={cn(
          "flex-1 min-w-0 pr-4",
          isSubSetting && "pl-4 border-l-2 border-[var(--border-subtle)]"
        )}
      >
        <Label
          htmlFor={id}
          className="text-sm font-medium text-[var(--text-primary)]"
        >
          {label}
        </Label>
        <p id={`${id}-desc`} className="text-xs text-[var(--text-muted)] mt-0.5">
          {description}
        </p>
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

// ============================================================================
// Checkbox Setting Row
// ============================================================================

interface CheckboxSettingRowProps {
  id: string;
  label: string;
  description: string;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
  isSubSetting?: boolean;
}

function CheckboxSettingRow({
  id,
  label,
  description,
  checked,
  disabled,
  onChange,
  isSubSetting = false,
}: CheckboxSettingRowProps) {
  return (
    <SettingRow
      id={id}
      label={label}
      description={description}
      isSubSetting={isSubSetting}
      isDisabled={disabled}
    >
      <Checkbox
        id={id}
        data-testid={id}
        checked={checked}
        onCheckedChange={onChange}
        disabled={disabled}
        aria-describedby={`${id}-desc`}
        className="data-[state=checked]:bg-[var(--accent-primary)] data-[state=checked]:border-[var(--accent-primary)]"
      />
    </SettingRow>
  );
}

// ============================================================================
// 3-State Override Select
// ============================================================================

type OverrideValue = "inherit" | "on" | "off";

const OVERRIDE_OPTIONS: { value: OverrideValue; label: string; description: string }[] = [
  { value: "inherit", label: "Inherit", description: "Use base policy" },
  { value: "on", label: "On", description: "Always enforce" },
  { value: "off", label: "Off", description: "Always bypass" },
];

function boolToOverride(value: boolean | null): OverrideValue {
  if (value === null) return "inherit";
  return value ? "on" : "off";
}

function overrideToBool(value: OverrideValue): boolean | null {
  if (value === "inherit") return null;
  return value === "on";
}

interface OverrideSelectRowProps {
  id: string;
  label: string;
  description: string;
  value: boolean | null;
  disabled: boolean;
  onChange: (value: boolean | null) => void;
}

function OverrideSelectRow({
  id,
  label,
  description,
  value,
  disabled,
  onChange,
}: OverrideSelectRowProps) {
  return (
    <SettingRow id={id} label={label} description={description} isSubSetting isDisabled={disabled}>
      <Select
        value={boolToOverride(value)}
        onValueChange={(v) => onChange(overrideToBool(v as OverrideValue))}
        disabled={disabled}
      >
        <SelectTrigger
          id={id}
          data-testid={id}
          aria-describedby={`${id}-desc`}
          className="w-[160px] bg-[var(--bg-surface)] border-[var(--border-default)] focus:ring-[var(--accent-primary)]"
        >
          <SelectValue placeholder="Select override" />
        </SelectTrigger>
        <SelectContent className="bg-[var(--bg-elevated)] border-[var(--border-default)]">
          {OVERRIDE_OPTIONS.map((opt) => (
            <SelectItem
              key={opt.value}
              value={opt.value}
              className="focus:bg-[var(--accent-muted)]"
            >
              <div className="flex flex-col">
                <span className="text-[var(--text-primary)]">{opt.label}</span>
                <span className="text-xs text-[var(--text-muted)]">{opt.description}</span>
              </div>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </SettingRow>
  );
}

// ============================================================================
// IdeationSettingsPanel Component
// ============================================================================

export type IdeationSettingsController = ReturnType<typeof useIdeationSettings>;

interface IdeationSettingsContentProps {
  controller: IdeationSettingsController;
  surface: "all" | "tasks" | "planning";
  embedded?: boolean;
}

export function IdeationSettingsContent({
  controller,
  surface,
  embedded = false,
}: IdeationSettingsContentProps) {
  const { settings, updateSettings, isLoading, isUpdating, updateError } = controller;
  const autoAcceptPlans = useUiStore((s) => s.autoAcceptPlans);
  const setAutoAcceptPlans = useUiStore((s) => s.setAutoAcceptPlans);
  const [showExternalOverrides, setShowExternalOverrides] = useState(false);

  const handleTasksEnabledChange = (checked: boolean) => {
    updateSettings({
      ...settings,
      tasksEnabled: checked,
    });
  };

  const handleRequireAcceptForFinalizeChange = (checked: boolean) => {
    updateSettings({
      ...settings,
      requireAcceptForFinalize: checked,
    });
  };

  const handleAutoVerifyPlansChange = (checked: boolean) => {
    updateSettings({
      ...settings,
      autoVerifyPlans: checked,
    });
  };

  const handleRequireVerificationForAcceptChange = (checked: boolean) => {
    updateSettings({
      ...settings,
      requireVerificationForAccept: checked,
    });
  };

  const handleExternalOverrideChange = (
    field: keyof ExternalIdeationOverrides,
    value: boolean | null
  ) => {
    updateSettings({
      ...settings,
      externalOverrides: {
        ...settings.externalOverrides,
        [field]: value,
      },
    });
  };

  const content = (
    <>
        {surface !== "planning" && (
          <>
        <CheckboxSettingRow
          id="enable-tasks"
          label="Enable Tasks"
          description="Off by default. Plans can still be implemented directly; attached Agent pipelines may finish, while active standalone Tasks are paused."
          checked={settings.tasksEnabled}
          disabled={isLoading || isUpdating}
          onChange={handleTasksEnabledChange}
        />
        {updateError instanceof Error &&
          updateError.message.includes("ralphx:tasks_drain_incomplete") && (
            <p role="alert" className="py-2 text-xs text-[var(--status-warning)]">
              Tasks is off. Some running Task processes could not be stopped yet and will be retried.
              <span className="block mt-1">{updateError.message}</span>
            </p>
          )}
        <CheckboxSettingRow id="require-accept-for-finalize" label="Require confirmation before finalizing" description="Pause finalize_proposals for user Accept/Reject before tasks are created" checked={settings.requireAcceptForFinalize} disabled={isUpdating} onChange={handleRequireAcceptForFinalizeChange} />
        <CheckboxSettingRow id="require-verification-for-accept" label="Require verification before accepting" description="The exact current plan artifact must have verification proof before it can be accepted" checked={settings.requireVerificationForAccept} disabled={isUpdating} onChange={handleRequireVerificationForAcceptChange} />
        <CheckboxSettingRow id="auto-accept-plans" label="Skip finalization confirmation" description="Automatically confirm all pending finalize dialogs without prompting (resets on app restart)" checked={autoAcceptPlans} disabled={false} onChange={setAutoAcceptPlans} />
          </>
        )}
        {surface !== "tasks" && (
          <CheckboxSettingRow id="auto-verify-plans" label="Verify automatically on acceptance" description="When verification is required, an acceptance attempt queues a visible Verify Plan turn instead of interrupting drafting" checked={settings.autoVerifyPlans} disabled={isUpdating} onChange={handleAutoVerifyPlansChange} />
        )}
        <div className="pt-1">
          <button type="button" data-testid="external-overrides-toggle" onClick={() => setShowExternalOverrides((v) => !v)} className="flex items-center gap-2 w-full py-2 text-left text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors">
            {showExternalOverrides ? <ChevronDown className="w-3.5 h-3.5" /> : <ChevronRight className="w-3.5 h-3.5" />}
            External Session Overrides
          </button>
          {showExternalOverrides && (
            <div className="space-y-1 mt-1">
              {surface !== "tasks" && <OverrideSelectRow id="ext-override-auto-verify-plans" label="Automatic verification on acceptance" description="Override acceptance-triggered Verify Plan turns for external sessions" value={settings.externalOverrides.autoVerifyPlans} disabled={isUpdating} onChange={(v) => handleExternalOverrideChange("autoVerifyPlans", v)} />}
              {surface !== "planning" && (
                <>
                  <OverrideSelectRow id="ext-override-verification-for-accept" label="Verification for accept" description="Override verification-before-accept gate for external sessions" value={settings.externalOverrides.requireVerificationForAccept} disabled={isUpdating} onChange={(v) => handleExternalOverrideChange("requireVerificationForAccept", v)} />
                  <OverrideSelectRow id="ext-override-accept-for-finalize" label="Accept before finalizing" description="Override accept-before-finalize gate for external sessions" value={settings.externalOverrides.requireAcceptForFinalize} disabled={isUpdating} onChange={(v) => handleExternalOverrideChange("requireAcceptForFinalize", v)} />
                </>
              )}
            </div>
          )}
        </div>
    </>
  );
  if (embedded) return content;
  return (
    <SectionCard
      icon={<ShieldCheck className="w-[18px] h-[18px] text-[var(--accent-primary)]" />}
      title={surface === "planning" ? "Planning" : "Tasks"}
      description={surface === "planning" ? "Configure automatic plan verification" : "Configure task and acceptance gates"}
    >
      {content}
    </SectionCard>
  );
}

export function IdeationSettingsPanel() {
  const controller = useIdeationSettings();
  return <IdeationSettingsContent controller={controller} surface="all" />;
}
