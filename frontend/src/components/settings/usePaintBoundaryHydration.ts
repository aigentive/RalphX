import { useEffect, useState } from "react";

import {
  cancelScheduledJob,
  scheduleAfterPaint,
} from "./SettingsDialog.performance";

/**
 * Paint-boundary gate (rule 24): true only after a frame + macrotask have passed.
 *
 * Shared rather than copied. Two settings panes now depend on "shell first, fetch
 * after"; a second private copy would let the two drift into different definitions of
 * when the boundary is crossed, which is exactly the property the rule-24 tests assert.
 */
export function usePaintBoundaryHydration(): boolean {
  const [hydrated, setHydrated] = useState(false);
  useEffect(() => {
    const job = scheduleAfterPaint(() => setHydrated(true));
    return () => cancelScheduledJob(job);
  }, []);
  return hydrated;
}
