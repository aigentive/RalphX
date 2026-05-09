/**
 * Tests for EffortEstimationPanel — hero estimate, range bar, team-level
 * presets, and collapsible methodology / calibration controls.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";

import { EffortEstimationPanel } from "./EffortEstimationPanel";
import { DEFAULT_METRICS_CONFIG } from "@/types/project-stats";
import type { MetricsConfig } from "@/types/project-stats";

// ---------------------------------------------------------------------------
// Hook mocks
// ---------------------------------------------------------------------------

const saveConfigMock = vi.fn();
let currentConfig: MetricsConfig | undefined = DEFAULT_METRICS_CONFIG;

vi.mock("@/hooks/useMetricsConfig", () => ({
  useMetricsConfig: () => ({ data: currentConfig }),
  useSaveMetricsConfig: () => ({ mutate: saveConfigMock }),
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

interface PanelOverrides {
  lowHours?: number;
  highHours?: number;
  taskCount?: number;
  earliestTaskDate?: string | null;
  latestTaskDate?: string | null;
  projectId?: string;
}

function renderPanel(overrides: PanelOverrides = {}) {
  return render(
    <EffortEstimationPanel
      lowHours={overrides.lowHours ?? 100}
      highHours={overrides.highHours ?? 200}
      taskCount={overrides.taskCount ?? 5}
      earliestTaskDate={
        overrides.earliestTaskDate === undefined
          ? "2026-01-01"
          : overrides.earliestTaskDate
      }
      latestTaskDate={
        overrides.latestTaskDate === undefined
          ? "2026-02-01"
          : overrides.latestTaskDate
      }
      projectId={overrides.projectId ?? "project-1"}
    />,
  );
}

beforeEach(() => {
  saveConfigMock.mockReset();
  currentConfig = DEFAULT_METRICS_CONFIG;
});

// ---------------------------------------------------------------------------
// Header / hero number
// ---------------------------------------------------------------------------

describe("EffortEstimationPanel header & hero", () => {
  it("renders header label and hero midpoint with units", () => {
    renderPanel({ lowHours: 100, highHours: 200 });
    expect(screen.getByText("Equivalent Developer Effort")).toBeInTheDocument();
    // Midpoint = 150
    expect(screen.getByText("~150")).toBeInTheDocument();
    expect(screen.getByText("developer hours")).toBeInTheDocument();
  });

  it("shows range labels (coding only / with overhead)", () => {
    renderPanel({ lowHours: 100, highHours: 200 });
    expect(screen.getByText("100h coding only")).toBeInTheDocument();
    expect(screen.getByText("200h with overhead")).toBeInTheDocument();
  });

  it("does NOT render customized badge when config matches defaults", () => {
    currentConfig = DEFAULT_METRICS_CONFIG;
    renderPanel();
    expect(screen.queryByText("customized")).not.toBeInTheDocument();
  });

  it("renders customized badge when config differs from defaults", () => {
    currentConfig = { ...DEFAULT_METRICS_CONFIG, simpleBaseHours: 7 };
    renderPanel();
    expect(screen.getByText("customized")).toBeInTheDocument();
  });

  it("falls back to DEFAULT_METRICS_CONFIG when hook data is undefined", () => {
    currentConfig = undefined;
    renderPanel();
    // Should still render hero without crashing and treat as default (no badge)
    expect(screen.queryByText("customized")).not.toBeInTheDocument();
    expect(screen.getByText("Equivalent Developer Effort")).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------------------
// FTE-months / context line / compression
// ---------------------------------------------------------------------------

describe("Context line", () => {
  it("shows FTE-months when midpoint >= 160h", () => {
    // midpoint = 240 → 1.5 FTE-months
    renderPanel({ lowHours: 200, highHours: 280 });
    expect(screen.getByText(/~1\.5 FTE-months/)).toBeInTheDocument();
  });

  it("hides FTE-months when midpoint < 160h", () => {
    // midpoint = 50
    renderPanel({ lowHours: 40, highHours: 60 });
    expect(screen.queryByText(/FTE-months/)).not.toBeInTheDocument();
  });

  it("renders singular 'task' when count is 1", () => {
    renderPanel({ taskCount: 1 });
    expect(screen.getByText("1 task")).toBeInTheDocument();
  });

  it("renders plural 'tasks' when count != 1", () => {
    renderPanel({ taskCount: 5 });
    expect(screen.getByText("5 tasks")).toBeInTheDocument();
  });

  it("renders date range when earliest and latest are provided", () => {
    renderPanel({
      earliestTaskDate: "2026-01-01",
      latestTaskDate: "2026-02-01",
    });
    // Date range uses an em-dash separator
    expect(screen.getByText(/Jan 2026.*Feb 2026/)).toBeInTheDocument();
  });

  it("renders single date when earliest equals latest", () => {
    renderPanel({
      earliestTaskDate: "2026-01-01",
      latestTaskDate: "2026-01-01",
    });
    expect(screen.getByText("Jan 2026")).toBeInTheDocument();
  });

  it("hides date range when earliestTaskDate is null", () => {
    renderPanel({ earliestTaskDate: null, latestTaskDate: "2026-02-01" });
    expect(screen.queryByText(/Jan 2026/)).not.toBeInTheDocument();
  });

  it("hides date range when latestTaskDate is null", () => {
    renderPanel({ earliestTaskDate: "2026-01-01", latestTaskDate: null });
    expect(screen.queryByText(/Jan 2026/)).not.toBeInTheDocument();
  });

  it("renders compression badge when ratio >= 2x", () => {
    // calendar weeks: 2026-01-01 → 2026-01-08 = ~1 week
    // midpoint hours: (400+600)/2 = 500 → 500/40 = 12.5 work-weeks → 13x compression
    renderPanel({
      lowHours: 400,
      highHours: 600,
      earliestTaskDate: "2026-01-01",
      latestTaskDate: "2026-01-08",
    });
    expect(screen.getByText(/x compression/)).toBeInTheDocument();
  });

  it("does not render compression when ratio < 2x", () => {
    renderPanel({
      lowHours: 10,
      highHours: 20,
      earliestTaskDate: "2026-01-01",
      latestTaskDate: "2026-02-01",
    });
    expect(screen.queryByText(/x compression/)).not.toBeInTheDocument();
  });

  it("does not render compression when calendar dates are null", () => {
    renderPanel({
      lowHours: 400,
      highHours: 600,
      earliestTaskDate: null,
      latestTaskDate: null,
    });
    expect(screen.queryByText(/x compression/)).not.toBeInTheDocument();
  });
});

// ---------------------------------------------------------------------------
// Team Level presets
// ---------------------------------------------------------------------------

describe("Team Level presets", () => {
  it("renders all four preset buttons", () => {
    renderPanel();
    expect(screen.getByText("Junior")).toBeInTheDocument();
    expect(screen.getByText("Mid")).toBeInTheDocument();
    expect(screen.getByText("Senior")).toBeInTheDocument();
    expect(screen.getByText("Staff+")).toBeInTheDocument();
  });

  it("does NOT show 'Custom values' when current config matches a preset", () => {
    currentConfig = DEFAULT_METRICS_CONFIG; // matches senior
    renderPanel();
    expect(screen.queryByText("Custom values")).not.toBeInTheDocument();
  });

  it("shows 'Custom values' when config does not match any preset", () => {
    currentConfig = { ...DEFAULT_METRICS_CONFIG, simpleBaseHours: 7 };
    renderPanel();
    expect(screen.getByText("Custom values")).toBeInTheDocument();
  });

  it("calls saveConfig with junior preset values when Junior is clicked", () => {
    renderPanel();
    fireEvent.click(screen.getByText("Junior"));
    expect(saveConfigMock).toHaveBeenCalledOnce();
    expect(saveConfigMock).toHaveBeenCalledWith(
      expect.objectContaining({
        simpleBaseHours: 4,
        mediumBaseHours: 10,
        complexBaseHours: 20,
        calendarFactor: 2.0,
      }),
    );
  });

  it("calls saveConfig with staff preset values when Staff+ is clicked", () => {
    renderPanel();
    fireEvent.click(screen.getByText("Staff+"));
    expect(saveConfigMock).toHaveBeenCalledWith(
      expect.objectContaining({
        simpleBaseHours: 0.5,
        complexBaseHours: 2,
        calendarFactor: 1.2,
      }),
    );
  });
});

// ---------------------------------------------------------------------------
// Methodology / calibration
// ---------------------------------------------------------------------------

describe("Methodology & calibration section", () => {
  it("hides calibration inputs by default (collapsed)", () => {
    renderPanel();
    expect(
      screen.queryByTestId("calibrate-simpleBaseHours"),
    ).not.toBeInTheDocument();
  });

  it("expands calibration when methodology toggle is clicked", () => {
    renderPanel();
    fireEvent.click(screen.getByText(/Methodology & calibration/));
    expect(screen.getByTestId("calibrate-simpleBaseHours")).toBeInTheDocument();
    expect(screen.getByTestId("calibrate-mediumBaseHours")).toBeInTheDocument();
    expect(screen.getByTestId("calibrate-complexBaseHours")).toBeInTheDocument();
    expect(screen.getByTestId("calibrate-calendarFactor")).toBeInTheDocument();
    expect(
      screen.getByTestId("calibrate-workingDaysPerWeek"),
    ).toBeInTheDocument();
  });

  it("re-collapses methodology when toggle is clicked again", () => {
    renderPanel();
    const toggle = screen.getByText(/Methodology & calibration/);
    fireEvent.click(toggle);
    expect(screen.getByTestId("calibrate-simpleBaseHours")).toBeInTheDocument();
    fireEvent.click(toggle);
    expect(
      screen.queryByTestId("calibrate-simpleBaseHours"),
    ).not.toBeInTheDocument();
  });

  it("renders 'What this captures' / 'NOT capture' explanatory copy when expanded", () => {
    renderPanel();
    fireEvent.click(screen.getByText(/Methodology & calibration/));
    expect(screen.getByText("What this captures")).toBeInTheDocument();
    expect(screen.getByText("What this does NOT capture")).toBeInTheDocument();
    expect(
      screen.getByText("These estimates are conservative by design."),
    ).toBeInTheDocument();
  });

  it("shows reset button only when config is customized", () => {
    currentConfig = { ...DEFAULT_METRICS_CONFIG, simpleBaseHours: 7 };
    renderPanel();
    fireEvent.click(screen.getByText(/Methodology & calibration/));
    expect(screen.getByTestId("calibration-reset")).toBeInTheDocument();
  });

  it("does not show reset button when config matches defaults", () => {
    currentConfig = DEFAULT_METRICS_CONFIG;
    renderPanel();
    fireEvent.click(screen.getByText(/Methodology & calibration/));
    expect(screen.queryByTestId("calibration-reset")).not.toBeInTheDocument();
  });

  it("calls saveConfig with DEFAULT_METRICS_CONFIG when reset is clicked", () => {
    currentConfig = { ...DEFAULT_METRICS_CONFIG, simpleBaseHours: 7 };
    renderPanel();
    fireEvent.click(screen.getByText(/Methodology & calibration/));
    fireEvent.click(screen.getByTestId("calibration-reset"));
    expect(saveConfigMock).toHaveBeenCalledWith(DEFAULT_METRICS_CONFIG);
  });
});

// ---------------------------------------------------------------------------
// Calibration field handlers
// ---------------------------------------------------------------------------

describe("Calibration field blur handlers", () => {
  it("saves a numeric simpleBaseHours change on blur", () => {
    renderPanel();
    fireEvent.click(screen.getByText(/Methodology & calibration/));
    const input = screen.getByTestId("calibrate-simpleBaseHours");
    fireEvent.change(input, { target: { value: "3" } });
    fireEvent.blur(input);
    expect(saveConfigMock).toHaveBeenCalledWith(
      expect.objectContaining({ simpleBaseHours: 3 }),
    );
  });

  it("saves calendarFactor change on blur", () => {
    renderPanel();
    fireEvent.click(screen.getByText(/Methodology & calibration/));
    const input = screen.getByTestId("calibrate-calendarFactor");
    fireEvent.change(input, { target: { value: "1.8" } });
    fireEvent.blur(input);
    expect(saveConfigMock).toHaveBeenCalledWith(
      expect.objectContaining({ calendarFactor: 1.8 }),
    );
  });

  it("ignores non-numeric blur value (does not save)", () => {
    renderPanel();
    fireEvent.click(screen.getByText(/Methodology & calibration/));
    const input = screen.getByTestId("calibrate-simpleBaseHours");
    fireEvent.change(input, { target: { value: "abc" } });
    fireEvent.blur(input);
    expect(saveConfigMock).not.toHaveBeenCalled();
  });

  it("clamps workingDaysPerWeek to [1, 7] (above range)", () => {
    renderPanel();
    fireEvent.click(screen.getByText(/Methodology & calibration/));
    const input = screen.getByTestId("calibrate-workingDaysPerWeek");
    fireEvent.change(input, { target: { value: "12" } });
    fireEvent.blur(input);
    expect(saveConfigMock).toHaveBeenCalledWith(
      expect.objectContaining({ workingDaysPerWeek: 7 }),
    );
  });

  it("clamps workingDaysPerWeek to [1, 7] (below range)", () => {
    renderPanel();
    fireEvent.click(screen.getByText(/Methodology & calibration/));
    const input = screen.getByTestId("calibrate-workingDaysPerWeek");
    fireEvent.change(input, { target: { value: "0" } });
    fireEvent.blur(input);
    expect(saveConfigMock).toHaveBeenCalledWith(
      expect.objectContaining({ workingDaysPerWeek: 1 }),
    );
  });

  it("rounds workingDaysPerWeek to nearest integer", () => {
    renderPanel();
    fireEvent.click(screen.getByText(/Methodology & calibration/));
    const input = screen.getByTestId("calibrate-workingDaysPerWeek");
    fireEvent.change(input, { target: { value: "4.6" } });
    fireEvent.blur(input);
    expect(saveConfigMock).toHaveBeenCalledWith(
      expect.objectContaining({ workingDaysPerWeek: 5 }),
    );
  });
});

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

describe("Edge cases", () => {
  it("handles highHours = 0 without dividing by zero (range bar fill = 0%)", () => {
    renderPanel({ lowHours: 0, highHours: 0 });
    // Component should still render hero with ~0 developer hours
    expect(screen.getByText("~0")).toBeInTheDocument();
    expect(screen.getByText("0h coding only")).toBeInTheDocument();
  });

  it("renders sovereignty footer", () => {
    renderPanel();
    expect(
      screen.getByText(/Computed locally\. Your metrics never leave your machine\./),
    ).toBeInTheDocument();
  });
});
