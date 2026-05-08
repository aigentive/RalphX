/**
 * Shared components and utilities for SettingsView
 *
 * Extracted from SettingsView.tsx to reduce file size and improve reusability.
 * Contains setting row components and section card.
 *
 * Note: Constants are in SettingsView.constants.ts to satisfy
 * react-refresh/only-export-components lint rule.
 */

import { useCallback, useRef } from "react";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import { Loader2, AlertCircle, X } from "lucide-react";
import { cn } from "@/lib/utils";

// Re-export constants from dedicated file
export { MODEL_OPTIONS } from "./SettingsView.constants";

// ============================================================================
// Saving Indicator Component
// ============================================================================

export function SavingIndicator() {
  return (
    <div className="flex items-center gap-2 px-3 py-1 rounded-full bg-[var(--bg-elevated)] text-[var(--text-muted)] text-sm">
      <Loader2 className="w-3.5 h-3.5 animate-spin" />
      <span>Saving...</span>
    </div>
  );
}

// ============================================================================
// Setting Row Component
// ============================================================================

export interface SettingRowProps {
  id: string;
  label: string;
  description: string;
  children: React.ReactNode;
  isSubSetting?: boolean;
  isDisabled?: boolean;
}

export function SettingRow({
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
        "settings-row",
        isDisabled && "opacity-50"
      )}
    >
      <div className={cn("min-w-0", isSubSetting && "pl-4")}>
        <Label
          htmlFor={id}
          className="settings-row__label"
        >
          {label}
        </Label>
        <p id={`${id}-desc`} className="settings-row__help">
          {description}
        </p>
      </div>
      <div className="settings-row__control">{children}</div>
    </div>
  );
}

// ============================================================================
// Toggle Setting Row
// ============================================================================

export interface ToggleSettingRowProps {
  id: string;
  label: string;
  description: string;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
  isSubSetting?: boolean;
}

export function ToggleSettingRow({
  id,
  label,
  description,
  checked,
  disabled,
  onChange,
  isSubSetting = false,
}: ToggleSettingRowProps) {
  // Radix Switch can emit a stale `onCheckedChange` when its `checked` prop
  // transitions externally. That spurious fire flips our controlled state
  // right back. Guard: only honour onCheckedChange when the user has actually
  // activated this switch. Use click activation, not pointer-down, so aborted
  // presses cannot leave a stale intent flag behind for the next external
  // prop transition.
  const userIntentRef = useRef(false);
  const markUserIntent = useCallback(() => {
    userIntentRef.current = true;
  }, []);
  const handleCheckedChange = useCallback(
    (next: boolean) => {
      if (!userIntentRef.current) return;
      userIntentRef.current = false;
      onChange(next);
    },
    [onChange],
  );
  return (
    <SettingRow
      id={id}
      label={label}
      description={description}
      isSubSetting={isSubSetting}
      isDisabled={disabled}
    >
      <Switch
        id={id}
        data-testid={id}
        checked={checked}
        onCheckedChange={handleCheckedChange}
        onClick={markUserIntent}
        disabled={disabled}
        aria-describedby={`${id}-desc`}
        className="settings-toggle data-[state=checked]:bg-[var(--accent-primary)]"
      />
    </SettingRow>
  );
}

// ============================================================================
// Number Input Setting Row
// ============================================================================

export interface NumberSettingRowProps {
  id: string;
  label: string;
  description: string;
  value: number;
  min: number;
  max: number;
  step: number;
  unit: string;
  disabled: boolean;
  onChange: (value: number) => void;
  isSubSetting?: boolean;
}

export function NumberSettingRow({
  id,
  label,
  description,
  value,
  min,
  max,
  step,
  unit,
  disabled,
  onChange,
  isSubSetting = false,
}: NumberSettingRowProps) {
  return (
    <SettingRow
      id={id}
      label={label}
      description={description}
      isSubSetting={isSubSetting}
      isDisabled={disabled}
    >
      <div className="settings-row__control">
        <Input
          type="number"
          id={id}
          data-testid={id}
          aria-describedby={`${id}-desc`}
          value={value}
          min={min}
          max={max}
          step={step}
          disabled={disabled}
          onChange={(e) => {
            const val = parseInt(e.target.value, 10);
            if (!isNaN(val) && val >= min && val <= max) {
              onChange(val);
            }
          }}
          className="settings-input w-20 text-right [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none"
        />
        {unit && (
          <span className="text-xs text-[var(--text-muted)]">{unit}</span>
        )}
      </div>
    </SettingRow>
  );
}

// ============================================================================
// Select Setting Row
// ============================================================================

export interface SelectOption<T extends string> {
  value: T;
  label: string;
  description: string;
}

export interface SelectSettingRowProps<T extends string> {
  id: string;
  label: string;
  description: string;
  value: T;
  options: SelectOption<T>[];
  disabled: boolean;
  onChange: (value: T) => void;
}

export function SelectSettingRow<T extends string>({
  id,
  label,
  description,
  value,
  options,
  disabled,
  onChange,
}: SelectSettingRowProps<T>) {
  const selected = options.find((opt) => opt.value === value);
  return (
    <SettingRow id={id} label={label} description={description} isDisabled={disabled}>
      <Select
        value={value}
        onValueChange={onChange}
        disabled={disabled}
      >
        <SelectTrigger
          id={id}
          data-testid={id}
          aria-describedby={`${id}-desc`}
          className="settings-select w-[200px] focus:ring-[var(--accent-primary)]"
        >
          {/* Styleguide §8: trigger shows label only; description is
              dropdown-item context, never a truncated second line in the trigger. */}
          <SelectValue placeholder="Select model">
            <span className="text-[var(--text-primary)]">{selected?.label ?? ""}</span>
          </SelectValue>
        </SelectTrigger>
        <SelectContent className="bg-[var(--bg-elevated)] border-[var(--border-default)]">
          {options.map((opt) => (
            <SelectItem
              key={opt.value}
              value={opt.value}
              className="focus:bg-[var(--accent-muted)]"
            >
              <div className="flex flex-col">
                <span className="text-[var(--text-primary)]">{opt.label}</span>
                <span className="text-xs text-[var(--text-muted)]">
                  {opt.description}
                </span>
              </div>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </SettingRow>
  );
}

// ============================================================================
// Section Card Component
// ============================================================================

export interface SectionCardProps {
  icon: React.ReactNode;
  title: string;
  description: string;
  children: React.ReactNode;
}

export function SectionCard({ icon, title, description, children }: SectionCardProps) {
  return (
    <div className="settings-section">
      <div className="settings-pane-head">
        <div className="settings-pane-head__icon p-2 rounded-lg shrink-0 [&>svg]:text-[var(--card-icon-color)]">
          {icon}
        </div>
        <div>
          <h3 className="settings-pane-head__title">
            {title}
          </h3>
          <p className="settings-pane-head__sub">{description}</p>
        </div>
      </div>
      <div className="settings-section__content">{children}</div>
    </div>
  );
}

// ============================================================================
// Skeleton Component
// ============================================================================

export function SettingsSkeleton() {
  return (
    <div
      data-testid="settings-skeleton"
      className="p-6 space-y-6 max-w-[720px] mx-auto"
    >
      {[1, 2, 3, 4, 5].map((i) => (
        <div key={i} className="p-5 rounded-[10px] bg-[var(--bg-elevated)] border border-[var(--border-default)]">
          <div className="flex items-center gap-3 mb-4">
            <Skeleton className="w-9 h-9 rounded-lg" />
            <div className="space-y-2">
              <Skeleton className="h-4 w-24" />
              <Skeleton className="h-3 w-40" />
            </div>
          </div>
          <Separator className="my-4" />
          <div className="space-y-4">
            {[1, 2, 3].map((j) => (
              <div key={j} className="flex justify-between items-center">
                <div className="space-y-1">
                  <Skeleton className="h-4 w-32" />
                  <Skeleton className="h-3 w-48" />
                </div>
                <Skeleton className="h-6 w-11 rounded-full" />
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

// ============================================================================
// Error Banner Component
// ============================================================================

export interface ErrorBannerProps {
  error: string;
  onDismiss: () => void;
}

export function ErrorBanner({ error, onDismiss }: ErrorBannerProps) {
  return (
    <div className="mx-6 mt-4 p-3 rounded-lg bg-[var(--status-error-muted)] border border-[var(--status-error-border)] flex items-center gap-3">
      <AlertCircle className="w-4 h-4 text-[var(--status-error)] shrink-0" />
      <p className="text-sm text-[var(--status-error)] flex-1">{error}</p>
      <Button
        variant="ghost"
        size="icon"
        onClick={onDismiss}
        className="h-6 w-6 hover:bg-[var(--status-error-border)]"
      >
        <X className="w-4 h-4 text-[var(--status-error)]" />
      </Button>
    </div>
  );
}
