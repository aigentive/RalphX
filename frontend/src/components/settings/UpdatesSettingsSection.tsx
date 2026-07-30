import { TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";

import type { UpdateChannel } from "@/api/update-channel";
import { useUpdateChannel } from "@/hooks/useUpdateChannel";

import { SettingsSection } from "./SettingsView.shared";

const UPDATE_CHANNEL_OPTIONS = [
  {
    value: "stable",
    label: "Stable — Recommended",
    description: "Tested/promoted releases; default.",
  },
  {
    value: "nightly",
    label: "Nightly — Early access",
    description:
      "Every daily prerelease; may contain regressions. Switching back to Stable stops future Nightly delivery but never downgrades the installed app; updates resume when Stable advances beyond it.",
  },
] as const satisfies ReadonlyArray<{
  value: UpdateChannel;
  label: string;
  description: string;
}>;

interface UpdateChannelCardProps {
  option: (typeof UPDATE_CHANNEL_OPTIONS)[number];
  selected: boolean;
  disabled: boolean;
  onSelect: (channel: UpdateChannel) => void;
}

function UpdateChannelCard({
  option,
  selected,
  disabled,
  onSelect,
}: UpdateChannelCardProps) {
  const descriptionId = `update-channel-${option.value}-description`;

  return (
    <button
      type="button"
      role="radio"
      aria-label={option.label}
      aria-checked={selected}
      aria-describedby={descriptionId}
      disabled={disabled}
      data-testid={`update-channel-${option.value}`}
      onClick={() => onSelect(option.value)}
      className="rounded-md px-3 py-3 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-primary)]"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: selected ? "var(--accent-primary)" : "var(--border-default)",
        borderStyle: "solid",
        borderWidth: selected ? 2 : 1,
      }}
    >
      <span className="flex items-start gap-3">
        <span
          aria-hidden="true"
          className="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-full"
          style={{
            backgroundColor: selected ? "var(--accent-primary)" : "var(--bg-base)",
            borderColor: selected ? "var(--accent-primary)" : "var(--border-default)",
            borderStyle: "solid",
            borderWidth: 1,
          }}
        >
          {selected ? <span className="h-1.5 w-1.5 rounded-full bg-white" /> : null}
        </span>
        <span className="min-w-0 space-y-1">
          <span className="block text-sm font-medium text-[var(--text-primary)]">
            {option.label}
          </span>
          <span
            id={descriptionId}
            className="block text-xs leading-5 text-[var(--text-muted)]"
          >
            {option.description}
          </span>
        </span>
      </span>
    </button>
  );
}

function UpdateChannelError({ children }: { children: ReactNode }) {
  return (
    <div
      role="alert"
      className="flex items-start gap-2 rounded-md border border-[var(--status-warning-border)] bg-[var(--bg-elevated)] px-3 py-2 text-xs text-[var(--status-warning)]"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: "var(--status-warning-border)",
        borderStyle: "solid",
        borderWidth: 1,
      }}
    >
      <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
      <p>{children}</p>
    </div>
  );
}

export function UpdatesSettingsSection() {
  const {
    updateChannel,
    isLoading,
    loadError,
    setUpdateChannel,
    isSaving,
    saveError,
  } = useUpdateChannel();

  return (
    <SettingsSection>
      <div
        role="radiogroup"
        aria-label="Update channel"
        className="grid gap-2"
      >
        {UPDATE_CHANNEL_OPTIONS.map((option) => (
          <UpdateChannelCard
            key={option.value}
            option={option}
            selected={updateChannel === option.value}
            disabled={isLoading || isSaving}
            onSelect={setUpdateChannel}
          />
        ))}
      </div>

      {loadError ? (
        <UpdateChannelError>
          Unable to load update channel. Stable is being used for update checks.
        </UpdateChannelError>
      ) : null}

      {saveError ? (
        <UpdateChannelError>
          Unable to save update channel. Please try again.
        </UpdateChannelError>
      ) : null}
    </SettingsSection>
  );
}
