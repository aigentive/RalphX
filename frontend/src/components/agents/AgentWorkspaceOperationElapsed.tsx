import { useEffect, useState } from "react";

import { formatElapsedTime } from "@/lib/formatters";

const ELAPSED_TICK_INTERVAL_MS = 1_000;

function elapsedSecondsSince(startedAtMs: number): number {
  return Math.max(0, Math.floor((Date.now() - startedAtMs) / 1_000));
}

export interface AgentWorkspaceOperationElapsedProps {
  startedAtMs: number | null;
}

export function AgentWorkspaceOperationElapsed({
  startedAtMs,
}: AgentWorkspaceOperationElapsedProps) {
  const [elapsedSeconds, setElapsedSeconds] = useState(() =>
    startedAtMs === null ? 0 : elapsedSecondsSince(startedAtMs),
  );

  useEffect(() => {
    if (startedAtMs === null) {
      return;
    }
    setElapsedSeconds(elapsedSecondsSince(startedAtMs));
    const intervalId = setInterval(() => {
      setElapsedSeconds(elapsedSecondsSince(startedAtMs));
    }, ELAPSED_TICK_INTERVAL_MS);
    return () => clearInterval(intervalId);
  }, [startedAtMs]);

  if (startedAtMs === null) {
    return null;
  }

  return <>{formatElapsedTime(elapsedSeconds)}</>;
}
