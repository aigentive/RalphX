export type TranscriptSnapshotRow = {
  kind: "user" | "text" | "tool" | "delegate";
  key: string;
  text: string;
};

function normalizedText(element: Element): string {
  return element.textContent?.replace(/\s+/g, " ").trim() ?? "";
}

function toolName(element: HTMLElement): string {
  const label = element
    .querySelector<HTMLElement>('[data-testid="tool-call-toggle"]')
    ?.getAttribute("aria-label") ?? "";
  const match = /^Tool call: ([^.]+)\./.exec(label);
  return (match?.[1] ?? "unknown").trim().toLowerCase();
}

function delegateIdentity(element: HTMLElement): string {
  const match = /replay-delegate-turn-[12]/.exec(normalizedText(element));
  return match?.[0] ?? normalizedText(element).toLowerCase();
}

function snapshotMessage(message: HTMLElement): TranscriptSnapshotRow[] {
  const rows: TranscriptSnapshotRow[] = [];
  message
    .querySelectorAll<HTMLElement>([
      '[data-testid="text-bubble-user"]',
      '[data-testid="text-bubble-assistant"]',
      '[data-testid="task-tool-call-card"]',
      '[data-testid="task-subagent-card"]',
      '[data-testid="diff-tool-call-view"]',
      '[data-testid="tool-call-indicator"]',
    ].join(", "))
    .forEach((element) => {
      if (element.matches('[data-testid="text-bubble-user"]')) {
        const text = normalizedText(element);
        if (text) rows.push({ kind: "user", key: `user:${text}`, text });
        return;
      }
      if (element.matches('[data-testid="text-bubble-assistant"]')) {
        const text = normalizedText(element);
        if (text) rows.push({ kind: "text", key: `text:${text}`, text });
        return;
      }
      if (element.matches(
        '[data-testid="task-tool-call-card"], [data-testid="task-subagent-card"]',
      )) {
        const identity = delegateIdentity(element);
        rows.push({ kind: "delegate", key: `delegate:${identity}`, text: identity });
        return;
      }
      if (element.matches('[data-testid="diff-tool-call-view"]')) {
        const diff = element;
        const path = normalizedText(
          diff.querySelector('[data-testid="diff-tool-call-file-path"]') ?? diff,
        );
        rows.push({ kind: "tool", key: `tool:edit:${path}`, text: path });
        return;
      }
      const name = toolName(element);
      rows.push({ kind: "tool", key: `tool:${name}`, text: name });
    });
  return rows;
}

/** Capture ordered user-visible blocks, retaining stable text/tool/delegation identities. */
export function captureTranscriptSnapshot(container: HTMLElement): TranscriptSnapshotRow[] {
  return Array.from(
    container.querySelectorAll<HTMLElement>('[data-chat-message-item="true"]'),
  ).flatMap(snapshotMessage);
}

function assertUniqueKeys(label: string, rows: readonly TranscriptSnapshotRow[]): void {
  const keys = new Set<string>();
  for (const row of rows) {
    if (keys.has(row.key)) {
      throw new Error(`${label} has duplicate transcript key: ${row.key}`);
    }
    keys.add(row.key);
  }
}

/** Exact parity includes order, identity, and text; it fails fast on duplicate identities. */
export function expectSameTranscript(
  actual: readonly TranscriptSnapshotRow[],
  expected: readonly TranscriptSnapshotRow[],
): void {
  assertUniqueKeys("actual transcript", actual);
  assertUniqueKeys("expected transcript", expected);
  if (actual.length !== expected.length) {
    throw new Error(`Transcript length mismatch: expected ${expected.length}, received ${actual.length}`);
  }
  for (let index = 0; index < expected.length; index += 1) {
    const actualRow = actual[index];
    const expectedRow = expected[index];
    if (
      actualRow?.kind !== expectedRow?.kind
      || actualRow?.key !== expectedRow?.key
      || actualRow?.text !== expectedRow?.text
    ) {
      throw new Error(
        `Transcript differs at row ${index}: expected ${JSON.stringify(expectedRow)}, received ${JSON.stringify(actualRow)}`,
      );
    }
  }
}

/** `prefix` must be an exact uninterrupted beginning of `full`, never a subsequence. */
export function expectPrefixExact(
  prefix: readonly TranscriptSnapshotRow[],
  full: readonly TranscriptSnapshotRow[],
): void {
  assertUniqueKeys("prefix transcript", prefix);
  assertUniqueKeys("full transcript", full);
  if (prefix.length > full.length) {
    throw new Error(`Prefix has ${prefix.length} rows but full transcript has ${full.length}`);
  }
  expectSameTranscript(full.slice(0, prefix.length), prefix);
}

/** A UI-side privacy regression probe: tool arguments must not bleed into text bubbles. */
export function textOnlyExposure(container: HTMLElement): string {
  return captureTranscriptSnapshot(container)
    .filter((row) => row.kind === "user" || row.kind === "text")
    .map((row) => row.text)
    .join("\n");
}
