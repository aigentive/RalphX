import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  dataRetentionApi,
  type DataRetentionPolicyInput,
  type DataRetentionSettingsResponse,
  type RetentionCycleReport,
  type SizeBudgetPreview,
} from "@/api/data-retention";
import { SettingsSection, SettingRow } from "./SettingsView.shared";
import { DataRetentionSizeBudgetDialog } from "./DataRetentionSizeBudgetDialog";
import { bytesToGib, formatBytes, formatTimestamp, gibToBytes } from "./settings-bytes";

function policyFrom(response: DataRetentionSettingsResponse): DataRetentionPolicyInput {
  const { settings } = response;
  return {
    enabled: settings.enabled,
    days: settings.days,
    archivedDays: settings.archivedDays,
    batchRows: settings.batchRows,
    sizeBudgetBytes: settings.sizeBudgetBytes,
    // Re-confirming an already-stored budget keeps consent attached to the row it belongs to.
    sizeBudgetConfirmed: settings.sizeBudgetBytes !== null,
  };
}

export function DataRetentionSection() {
  const [response, setResponse] = useState<DataRetentionSettingsResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [budgetGib, setBudgetGib] = useState("");
  const [preview, setPreview] = useState<SizeBudgetPreview | null>(null);
  const [pendingBudgetBytes, setPendingBudgetBytes] = useState<number | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [runConfirm, setRunConfirm] = useState(false);
  const [running, setRunning] = useState(false);
  const [report, setReport] = useState<RetentionCycleReport | null>(null);

  const load = async () => {
    try {
      setError(null);
      const loaded = await dataRetentionApi.getSettings();
      setResponse(loaded);
      setBudgetGib(String(bytesToGib(
        loaded.settings.sizeBudgetBytes ?? loaded.recommendedSizeBudgetBytes,
      )));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to load retention settings");
    }
  };
  useEffect(() => { void load(); }, []);

  const save = async (patch: Partial<DataRetentionPolicyInput>) => {
    if (!response) return;
    setSaving(true);
    try {
      setError(null);
      setResponse(await dataRetentionApi.updateSettings({ ...policyFrom(response), ...patch }));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to save retention settings");
    } finally {
      setSaving(false);
    }
  };

  const settings = response?.settings ?? null;
  const budgetBytes = Number(budgetGib) > 0 ? gibToBytes(Number(budgetGib)) : 0;
  const currentBudget = settings?.sizeBudgetBytes ?? null;
  // Lowering a cap deletes strictly more, so it needs the same preview + consent.
  const needsConfirmation = currentBudget === null || budgetBytes < currentBudget;

  const applyBudget = async () => {
    if (!budgetBytes) return;
    if (!needsConfirmation) {
      await save({ sizeBudgetBytes: budgetBytes, sizeBudgetConfirmed: true });
      return;
    }
    setPreviewing(true);
    try {
      setError(null);
      setPreview(await dataRetentionApi.previewSizeBudget(budgetBytes));
      setPendingBudgetBytes(budgetBytes);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to preview the size limit");
    } finally {
      setPreviewing(false);
    }
  };

  const confirmBudget = async () => {
    const bytes = pendingBudgetBytes;
    setPendingBudgetBytes(null);
    setPreview(null);
    if (bytes) await save({ sizeBudgetBytes: bytes, sizeBudgetConfirmed: true });
  };

  const runNow = async () => {
    setRunning(true);
    try {
      setError(null);
      setReport(await dataRetentionApi.runNow());
      await load();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to run cleanup");
    } finally {
      setRunning(false);
      setRunConfirm(false);
    }
  };

  const sizeBudgetActive = Boolean(settings?.sizeBudgetBytes && settings?.sizeBudgetConfirmedAt);

  return <>
    <SettingsSection>
      {error ? <p role="alert" className="text-sm text-destructive">{error}</p> : null}

      {settings?.sizeBudgetAdvised && !sizeBudgetActive ? (
        <p
          data-testid="size-budget-advisory"
          className="text-sm rounded-[8px] px-3 py-2 bg-[var(--bg-elevated)] text-[var(--text-secondary)]"
        >
          Tool-call detail is using {formatBytes(settings.lastRunPayloadBytes ?? 0)}. Time-based
          cleanup cannot reach data newer than {settings.days} days — set a size limit below to
          reclaim it, then compact the database.
        </p>
      ) : null}

      <SettingRow
        id="retention-enabled"
        label="Automatic cleanup"
        description="Periodically delete stored tool-call detail. Message text, previews, and the conversation timeline always stay."
      >
        <Switch
          id="retention-enabled"
          checked={settings?.enabled ?? false}
          disabled={saving || !settings}
          onCheckedChange={(checked) => void save({ enabled: checked })}
        />
      </SettingRow>

      <SettingRow
        id="retention-days"
        label="Keep tool-call detail for"
        description="Days of detail kept for active conversations."
      >
        <Input
          id="retention-days" type="number" min={1} className="w-24"
          defaultValue={settings?.days ?? 90} disabled={saving || !settings}
          onBlur={(event) => {
            const days = Number(event.currentTarget.value);
            if (days >= 1 && days !== settings?.days) void save({ days });
          }}
        />
      </SettingRow>

      <SettingRow
        id="retention-archived-days"
        label="Archived conversations"
        description="Days of detail kept once a conversation is archived."
      >
        <Input
          id="retention-archived-days" type="number" min={1} className="w-24"
          defaultValue={settings?.archivedDays ?? 7} disabled={saving || !settings}
          onBlur={(event) => {
            const archivedDays = Number(event.currentTarget.value);
            if (archivedDays >= 1 && archivedDays !== settings?.archivedDays) {
              void save({ archivedDays });
            }
          }}
        />
      </SettingRow>

      <SettingRow
        id="retention-size-budget"
        label="Size limit for tool-call detail"
        description={sizeBudgetActive
          ? `Oldest detail is deleted to keep storage under ${formatBytes(currentBudget ?? 0)} — including detail still inside the time window above.`
          : "Off. RalphX keeps every tool-call detail inside the time window above. A size limit also deletes detail that is still inside that window."}
      >
        <div className="flex items-center gap-2">
          <Input
            id="retention-size-budget" type="number" min={1} step={1} className="w-24"
            value={budgetGib} disabled={saving || previewing || !settings}
            onChange={(event) => setBudgetGib(event.currentTarget.value)}
            aria-label="Size limit in GB"
          />
          <span className="text-sm text-[var(--text-muted)]">GB</span>
          <Button
            variant={sizeBudgetActive ? "outline" : "default"}
            disabled={saving || previewing || !settings || !budgetBytes}
            onClick={() => void applyBudget()}
          >
            {previewing ? <Loader2 className="h-4 w-4 animate-spin" /> : sizeBudgetActive ? "Update limit" : "Enable size limit…"}
          </Button>
          {sizeBudgetActive ? (
            <Button
              variant="ghost" disabled={saving}
              onClick={() => void save({ sizeBudgetBytes: null, sizeBudgetConfirmed: false })}
            >
              Turn off
            </Button>
          ) : null}
        </div>
      </SettingRow>

      <SettingRow
        id="retention-last-run"
        label="Last cleanup"
        description="Measured when cleanup last ran — Settings never scans your database on open."
      >
        <span data-testid="retention-last-run" className="text-sm text-[var(--text-secondary)]">
          {settings?.lastRunAt
            ? `${formatTimestamp(settings.lastRunAt)} · ${(settings.lastRunPrunedRows ?? 0).toLocaleString()} records removed · ${formatBytes(settings.lastRunPayloadBytes ?? 0)} stored`
            : "Never"}
        </span>
      </SettingRow>

      <SettingRow
        id="retention-run-now"
        label="Run cleanup now"
        description={sizeBudgetActive
          ? "Applies the time window and the size limit immediately."
          : "Removes only tool-call detail older than the time window above."}
      >
        <Button disabled={running || !settings} onClick={() => setRunConfirm(true)}>
          {running ? <Loader2 className="h-4 w-4 animate-spin" /> : "Run cleanup now"}
        </Button>
      </SettingRow>

      {report ? (
        <p data-testid="retention-run-report" className="text-sm text-[var(--text-secondary)]">
          Removed {report.prunedRows.toLocaleString()} records.
          {report.compactionRecommended
            ? " Deleted data still occupies the database file — schedule a compaction below to hand that space back to your disk."
            : ""}
        </p>
      ) : null}

      <SettingRow
        id="retention-batch-rows"
        label="Advanced: rows per batch"
        description="Lower values hold the database lock for shorter periods during cleanup."
      >
        <Input
          id="retention-batch-rows" type="number" min={1} max={10000} className="w-24"
          defaultValue={settings?.batchRows ?? 500} disabled={saving || !settings}
          onBlur={(event) => {
            const batchRows = Number(event.currentTarget.value);
            if (batchRows >= 1 && batchRows !== settings?.batchRows) void save({ batchRows });
          }}
        />
      </SettingRow>
    </SettingsSection>

    <DataRetentionSizeBudgetDialog
      open={pendingBudgetBytes !== null}
      preview={preview}
      budgetBytes={pendingBudgetBytes ?? 0}
      onOpenChange={(open) => { if (!open) { setPendingBudgetBytes(null); setPreview(null); } }}
      onConfirm={() => void confirmBudget()}
    />

    <AlertDialog open={runConfirm} onOpenChange={setRunConfirm}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Run cleanup now?</AlertDialogTitle>
          <AlertDialogDescription>
            {sizeBudgetActive
              ? "RalphX will delete tool-call detail outside the time window and any oldest detail above the size limit. Message text, previews, and the timeline stay."
              : "RalphX will delete tool-call detail older than the time window. Nothing inside the window is touched. Message text, previews, and the timeline stay."}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction onClick={(event) => { event.preventDefault(); void runNow(); }}>
            Run cleanup
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  </>;
}
