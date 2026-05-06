import { useState } from "react";
import { Check, ChevronDown, Contrast, Moon, SunMedium, type LucideIcon } from "lucide-react";

import { cn } from "@/lib/utils";
import { useThemeStore, type ThemeName } from "@/stores/themeStore";

interface ThemeOption {
  value: ThemeName;
  label: string;
  shortLabel: string;
  description: string;
  icon: LucideIcon;
}

const THEME_OPTIONS: ThemeOption[] = [
  {
    value: "dark",
    label: "Dark",
    shortLabel: "Dark",
    description: "Default RalphX palette with subdued surfaces and warm orange accents.",
    icon: Moon,
  },
  {
    value: "light",
    label: "Light",
    shortLabel: "Light",
    description: "Warm off-white surfaces with the same orange accent family.",
    icon: SunMedium,
  },
  {
    value: "high-contrast",
    label: "High contrast",
    shortLabel: "Contrast",
    description: "Maximum-contrast black canvas with shape-first emphasis and stronger borders.",
    icon: Contrast,
  },
];

interface ThemeSelectorProps {
  className?: string;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}

export function ThemeSelector({
  className = "",
  open: controlledOpen,
  onOpenChange,
}: ThemeSelectorProps) {
  const theme = useThemeStore((s) => s.theme);
  const setTheme = useThemeStore((s) => s.setTheme);
  const [uncontrolledOpen, setUncontrolledOpen] = useState(false);
  const open = controlledOpen ?? uncontrolledOpen;
  const setOpen = onOpenChange ?? setUncontrolledOpen;
  const current = THEME_OPTIONS.find((option) => option.value === theme) ?? THEME_OPTIONS[0]!;
  const CurrentIcon = current.icon;

  return (
    <div
      className={cn(
        "relative inline-flex shrink-0",
        className
      )}
      data-testid="theme-selector"
    >
      <button
        type="button"
        className="inline-flex h-8 items-center gap-1.5 rounded-[6px] border px-2.5 text-[0.7812rem] font-medium transition-colors duration-150 outline-none hover:border-[var(--border-strong)] hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
        style={{
          backgroundColor: open ? "var(--bg-hover)" : "var(--bg-elevated)",
          borderColor: open ? "var(--border-strong)" : "var(--border-default)",
          color: open ? "var(--text-primary)" : "var(--text-secondary)",
        }}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={`Theme · ${current.label}`}
        data-testid="theme-selector-trigger"
        onClick={() => setOpen(!open)}
      >
        <CurrentIcon className="h-3.5 w-3.5 shrink-0" style={{ color: "var(--text-muted)" }} />
        <span>{current.label}</span>
        <ChevronDown
          className={cn("h-3 w-3 shrink-0 transition-transform duration-150", open && "rotate-180")}
          style={{ color: open ? "var(--text-secondary)" : "var(--text-muted)" }}
        />
      </button>

      {open && (
        <div
          role="menu"
          aria-label="Theme"
          data-testid="theme-selector-menu"
          className="absolute right-0 top-[calc(100%+6px)] z-[60] flex min-w-[168px] flex-col gap-px rounded-[8px] border p-1 shadow-lg"
          style={{
            backgroundColor: "var(--bg-elevated)",
            borderColor: "var(--border-default)",
            boxShadow: "var(--shadow-lg)",
          }}
        >
          {THEME_OPTIONS.map((option) => {
            const OptionIcon = option.icon;
            const isActive = option.value === theme;

            return (
              <button
                key={option.value}
                type="button"
                role="menuitemradio"
                aria-checked={isActive}
                aria-label={`${option.label} theme`}
                onClick={() => {
                  setTheme(option.value);
                  setOpen(false);
                }}
                className="inline-flex items-center gap-2.5 rounded-[6px] px-2 py-1.5 text-left text-[0.7812rem] font-medium transition-colors duration-150 outline-none hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
                style={{ color: isActive ? "var(--text-primary)" : "var(--text-secondary)" }}
                data-testid={`theme-option-${option.value}`}
              >
                <OptionIcon
                  className="h-3.5 w-3.5 shrink-0"
                  style={{ color: isActive ? "var(--accent-primary)" : "var(--text-muted)" }}
                />
                <span className="flex-1">{option.label}</span>
                <Check
                  className="h-3 w-3 shrink-0"
                  style={{
                    color: "var(--accent-primary)",
                    opacity: isActive ? 1 : 0,
                  }}
                />
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
