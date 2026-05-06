/**
 * TeamConfigPanel - Configuration panel for team mode ideation sessions
 *
 * Shows when Research or Debate team mode is selected.
 * Allows configuring max teammates, model ceiling, budget, and composition mode.
 */

import type { TeamConfig, CompositionMode } from "@/types/ideation";

interface TeamConfigPanelProps {
  config: TeamConfig;
  onChange: (config: TeamConfig) => void;
}

const MAX_TEAMMATES_OPTIONS = [2, 3, 4, 5, 6, 7, 8];
const MODEL_OPTIONS = [
  { value: "haiku", label: "Haiku" },
  { value: "sonnet", label: "Sonnet" },
  { value: "opus", label: "Opus" },
];
const BUDGET_OPTIONS = [
  { value: "", label: "None" },
  { value: "5", label: "$5" },
  { value: "10", label: "$10" },
  { value: "25", label: "$25" },
];

const selectStyle: React.CSSProperties = {
  background: "var(--overlay-faint)",
  border: "1px solid var(--overlay-weak)",
  color: "var(--text-primary)",
  borderRadius: "8px",
  padding: "6px 10px",
  fontSize: "0.8125rem",
  outline: "none",
};

export function TeamConfigPanel({ config, onChange }: TeamConfigPanelProps) {
  const update = (partial: Partial<TeamConfig>) => {
    onChange({ ...config, ...partial });
  };

  return (
    <div
      className="w-full rounded-xl p-4 mt-4 text-left"
      style={{
        background: "var(--overlay-faint)",
        border: "1px solid var(--overlay-faint)",
      }}
    >
      {/* Row 1: Max teammates + Model ceiling */}
      <div className="flex items-center gap-4 mb-3">
        <label className="flex items-center gap-2 text-[0.8125rem]" style={{ color: "var(--text-secondary)" }}>
          <span>Max teammates:</span>
          <select
            value={config.maxTeammates}
            onChange={(e) => update({ maxTeammates: Number(e.target.value) })}
            style={selectStyle}
          >
            {MAX_TEAMMATES_OPTIONS.map((n) => (
              <option key={n} value={n}>{n}</option>
            ))}
          </select>
        </label>

        <label className="flex items-center gap-2 text-[0.8125rem]" style={{ color: "var(--text-secondary)" }}>
          <span>Model ceiling:</span>
          <select
            value={config.modelCeiling}
            onChange={(e) => update({ modelCeiling: e.target.value })}
            style={selectStyle}
          >
            {MODEL_OPTIONS.map((m) => (
              <option key={m.value} value={m.value}>{m.label}</option>
            ))}
          </select>
        </label>
      </div>

      {/* Row 2: Budget + Composition */}
      <div className="flex items-center gap-4 mb-3">
        <label className="flex items-center gap-2 text-[0.8125rem]" style={{ color: "var(--text-secondary)" }}>
          <span>Budget limit:</span>
          <select
            value={config.budgetLimit ?? ""}
            onChange={(e) => update({ budgetLimit: e.target.value ? Number(e.target.value) : undefined })}
            style={selectStyle}
          >
            {BUDGET_OPTIONS.map((b) => (
              <option key={b.value} value={b.value}>{b.label}</option>
            ))}
          </select>
        </label>

        <div className="flex items-center gap-3 text-[0.8125rem]" style={{ color: "var(--text-secondary)" }}>
          <span>Composition:</span>
          <CompositionRadio
            value={config.compositionMode}
            onChange={(mode) => update({ compositionMode: mode })}
          />
        </div>
      </div>

      {/* Constrained preset roles */}
      {config.compositionMode === "constrained" && (
        <div className="mt-3 pl-1">
          <p
            className="text-[0.75rem] mb-1.5"
            style={{ color: "var(--text-secondary)" }}
          >
            Available specialist roles:
          </p>
          <div className="flex flex-col gap-1 ml-1">
            <span className="text-[0.75rem]" style={{ color: "var(--text-secondary)" }}>
              ✓ researcher <span style={{ color: "var(--text-muted)" }}>(codebase research)</span>
            </span>
            <span className="text-[0.75rem]" style={{ color: "var(--text-secondary)" }}>
              ✓ critic <span style={{ color: "var(--text-muted)" }}>(adversarial stress-testing)</span>
            </span>
          </div>
          <p
            className="text-[0.6875rem] mt-1.5"
            style={{ color: "var(--text-muted)" }}
          >
            ⓘ Lead will select from these roles only.
          </p>
        </div>
      )}
    </div>
  );
}

function CompositionRadio({
  value,
  onChange,
}: {
  value: CompositionMode;
  onChange: (v: CompositionMode) => void;
}) {
  const options: { value: CompositionMode; label: string }[] = [
    { value: "dynamic", label: "Dynamic" },
    { value: "constrained", label: "Constrained" },
  ];

  return (
    <div className="flex items-center gap-3">
      {options.map((opt) => (
        <label key={opt.value} className="flex items-center gap-1.5 cursor-pointer" onClick={() => onChange(opt.value)}>
          <span
            className="w-3.5 h-3.5 rounded-full border flex items-center justify-center"
            style={{
              borderColor: value === opt.value ? "var(--accent-primary)" : "var(--overlay-moderate)",
              background: value === opt.value ? "var(--accent-primary)" : "transparent",
            }}
          >
            {value === opt.value && (
              <span
                className="w-1.5 h-1.5 rounded-full"
                style={{ background: "var(--text-inverse)" }}
              />
            )}
          </span>
          <span style={{ color: value === opt.value ? "var(--text-primary)" : "var(--text-secondary)" }}>
            {opt.label}
          </span>
        </label>
      ))}
    </div>
  );
}
