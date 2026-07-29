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
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useIdeationSettings } from "@/hooks/useIdeationSettings";
import { useConfirmation } from "@/hooks/useConfirmation";
import { useUiStore } from "@/stores/uiStore";
import type { ExternalIdeationOverrides } from "@/types/ideation-config";
import { SectionCard, SettingRow, ToggleSettingRow } from "./SettingsView.shared";

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
  const {
    settings,
    updateSettings,
    fetchTasksDisableImpact,
    setTasksEnabled,
    isLoading,
    isUpdating,
    updateError,
  } = controller;
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const autoAcceptPlans = useUiStore((s) => s.autoAcceptPlans);
  const setAutoAcceptPlans = useUiStore((s) => s.setAutoAcceptPlans);
  const [showExternalOverrides, setShowExternalOverrides] = useState(false);

  const handleTasksEnabledChange = (checked: boolean) => {
    if (checked) {
      void setTasksEnabled(true);
      return;
    }
    void confirm({
      title: "Turn Tasks off?",
      description: "Checking task-managed work before shutdown…",
      confirmText: "Pause task-managed work and turn Tasks off",
      pendingText: "Pausing task-managed work…",
      variant: "destructive",
      prepare: async () => {
        const impact = await fetchTasksDisableImpact();
        return {
          description: `${impact.activeStandaloneTasks} active standalone task${impact.activeStandaloneTasks === 1 ? "" : "s"} and ${impact.activeAttachedAgentWorkspaces} attached Agent workspace${impact.activeAttachedAgentWorkspaces === 1 ? "" : "s"} will be paused. ${impact.activeBranchUpdateOperations} active branch update operation${impact.activeBranchUpdateOperations === 1 ? "" : "s"} will be fenced. Task history and worktrees are retained; new, restart, and resume actions are blocked. Direct Agent implementation remains available, and re-enabling Tasks will not resume paused work automatically.`,
        };
      },
      onConfirm: () => setTasksEnabled(false),
      recoverFromError: (error) => {
        if (
          error instanceof Error &&
          error.message.includes("ralphx:tasks_drain_incomplete")
        ) {
          return {
            title: "Tasks shutdown is incomplete",
            description: `${error.message} Task-managed work remains fenced while shutdown is retried.`,
            confirmText: "Retry shutdown",
          };
        }
        return null;
      },
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

  const handleAutoVerifyDraftPlansChange = (checked: boolean) => {
    updateSettings({
      ...settings,
      autoVerifyDraftPlans: checked,
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
        <ToggleSettingRow
          id="enable-tasks"
          label="Enable Tasks"
          description="Off by default. Disabling pauses all task-managed work immediately; history and worktrees are retained, and plans can still be implemented directly."
          checked={settings.tasksEnabled}
          disabled={
            isLoading || isUpdating || settings.tasksFeatureState === "draining"
          }
          onChange={handleTasksEnabledChange}
        />
        {settings.tasksFeatureState === "draining" && (
          <div
            role="alert"
            className="flex items-center justify-between gap-3 py-2 text-xs text-[var(--status-warning)]"
          >
            <span>
              Tasks shutdown is incomplete. Task-managed work remains fenced until cleanup
              succeeds.
              {updateError instanceof Error && (
                <span className="block mt-1">{updateError.message}</span>
              )}
            </span>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={isUpdating}
              onClick={() => void setTasksEnabled(false)}
            >
              Retry shutdown
            </Button>
          </div>
        )}
        {/* Require agent confirmation before finalizing proposals */}
        <ToggleSettingRow
          id="require-accept-for-finalize"
          label="Require confirmation before finalizing"
          description="Pause plan finalization for user Accept or Reject before tasks are created."
          checked={settings.requireAcceptForFinalize}
          disabled={isUpdating}
          onChange={handleRequireAcceptForFinalizeChange}
        />

        <ToggleSettingRow
          id="require-verification-for-accept"
          label="Require verification before accepting"
          description="The exact current plan artifact must have verification proof before it can be accepted."
          checked={settings.requireVerificationForAccept}
          disabled={isUpdating}
          onChange={handleRequireVerificationForAcceptChange}
        />

        {/* Auto-accept finalization dialogs (in-memory only) */}
        <ToggleSettingRow
          id="auto-accept-plans"
          label="Skip finalization confirmation"
          description="Automatically confirm all pending finalize dialogs without prompting. Resets on app restart."
          checked={autoAcceptPlans}
          disabled={false}
          onChange={setAutoAcceptPlans}
        />
          </>
        )}
        {surface !== "tasks" && (
          <>
            <ToggleSettingRow
              id="auto-verify-draft-plans"
              label="Verify draft plans automatically"
              description="After a successful Plan-mode Agent response, queue a visible Verify Plan turn in the same conversation."
              checked={settings.autoVerifyDraftPlans}
              disabled={isUpdating}
              onChange={handleAutoVerifyDraftPlansChange}
            />
            <ToggleSettingRow
              id="auto-verify-plans"
              label="Queue missing verification on acceptance"
              description="When verification is required, an acceptance attempt queues a visible Verify Plan turn."
              checked={settings.autoVerifyPlans}
              disabled={isUpdating}
              onChange={handleAutoVerifyPlansChange}
            />
          </>
        )}
        <div className="pt-1">
          <button type="button" data-testid="external-overrides-toggle" onClick={() => setShowExternalOverrides((v) => !v)} className="flex items-center gap-2 w-full py-2 text-left text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors">
            {showExternalOverrides ? <ChevronDown className="w-3.5 h-3.5" /> : <ChevronRight className="w-3.5 h-3.5" />}
            External Session Overrides
          </button>
          {showExternalOverrides && (
            <div className="space-y-1 mt-1">
              {surface !== "tasks" && <OverrideSelectRow id="ext-override-auto-verify-plans" label="Automatic verification on acceptance" description="Override acceptance-triggered Verify Plan turns for external sessions." value={settings.externalOverrides.autoVerifyPlans} disabled={isUpdating} onChange={(v) => handleExternalOverrideChange("autoVerifyPlans", v)} />}
              {surface !== "planning" && (
                <>
                  <OverrideSelectRow id="ext-override-verification-for-accept" label="Verification for accept" description="Override the verification-before-accept gate for external sessions." value={settings.externalOverrides.requireVerificationForAccept} disabled={isUpdating} onChange={(v) => handleExternalOverrideChange("requireVerificationForAccept", v)} />
                  <OverrideSelectRow id="ext-override-accept-for-finalize" label="Accept before finalizing" description="Override the accept-before-finalize gate for external sessions." value={settings.externalOverrides.requireAcceptForFinalize} disabled={isUpdating} onChange={(v) => handleExternalOverrideChange("requireAcceptForFinalize", v)} />
                </>
              )}
            </div>
          )}
        </div>
    </>
  );
  const body = (
    <>
      {content}
      <ConfirmationDialog {...confirmationDialogProps} />
    </>
  );
  if (embedded) return body;
  return (
    <SectionCard
      icon={<ShieldCheck className="w-[18px] h-[18px] text-[var(--accent-primary)]" />}
      title={surface === "planning" ? "Planning" : "Tasks"}
      description={surface === "planning" ? "Configure automatic plan verification" : "Configure task and acceptance gates"}
    >
      {body}
    </SectionCard>
  );
}

export function IdeationSettingsPanel() {
  const controller = useIdeationSettings();
  return <IdeationSettingsContent controller={controller} surface="all" />;
}
