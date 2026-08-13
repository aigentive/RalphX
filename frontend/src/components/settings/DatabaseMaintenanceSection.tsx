import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { databaseMaintenanceApi, type DatabaseMaintenanceStats } from "@/api/database-maintenance";
import { SettingsSection, SettingRow } from "./SettingsView.shared";
import { formatBytes, formatTimestamp } from "./settings-bytes";

/** Share of the file that must be reclaimable before compaction is worth recommending. */
const COMPACTION_RECOMMENDED_SHARE = 0.2;

const SKIP_REASON_COPY: Record<string, string> = {
  insufficient_disk_headroom: "Not enough free disk space",
  disk_headroom_unavailable: "Free disk space could not be checked",
  database_missing: "No database file was found",
  database_above_auto_limit: "Database is above the automatic size limit — schedule it manually",
  freelist_below_auto_limit: "Too little reclaimable space to be worth it",
  wal_checkpoint_incomplete: "The database was still being written to",
  swap_interrupted:
    "RalphX stopped while swapping in the compacted database — the original is in the backup folder",
};

function describeLastCompaction(stats: DatabaseMaintenanceStats): string | null {
  const record = stats.lastCompaction;
  if (!record) return null;
  const when = formatTimestamp(record.atRfc3339);
  if (record.outcome === "compacted") {
    return `Compacted ${when} · ${formatBytes(record.reclaimedBytes ?? 0)} reclaimed`;
  }
  const reason = record.reason ? SKIP_REASON_COPY[record.reason] ?? record.reason : "Unknown reason";
  return record.outcome === "skipped" ? `Skipped ${when} · ${reason}` : `Failed ${when} · ${reason}`;
}

export function DatabaseMaintenanceSection() {
  const [stats, setStats] = useState<DatabaseMaintenanceStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [saving, setSaving] = useState(false);
  const load = async () => {
    try { setError(null); setStats(await databaseMaintenanceApi.getStats()); }
    catch (cause) { setError(cause instanceof Error ? cause.message : "Unable to load database maintenance status"); }
  };
  useEffect(() => { void load(); }, []);
  const setPending = async (pending: boolean) => {
    setSaving(true);
    try { setError(null); await databaseMaintenanceApi.setPending(pending); await load(); }
    catch (cause) { setError(cause instanceof Error ? cause.message : "Unable to update database compaction request"); }
    finally { setSaving(false); setConfirming(false); }
  };
  const lastCompaction = stats ? describeLastCompaction(stats) : null;
  // Deleted rows return pages to the freelist and never shrink the file, so without this
  // the retention work reclaims space the user never actually gets back.
  const compactionRecommended = Boolean(
    stats && stats.databaseBytes > 0
      && stats.reclaimableBytes / stats.databaseBytes >= COMPACTION_RECOMMENDED_SHARE,
  );
  return <>
    <SettingsSection>
      {error ? <p role="alert" className="text-sm text-destructive">{error}</p> : null}
      <SettingRow id="database-size" label="Database size" description="Current local RalphX database footprint.">
        <span data-testid="database-size">{stats ? formatBytes(stats.databaseBytes) : "Loading…"}</span>
      </SettingRow>
      <SettingRow id="database-reclaimable" label="Estimated reclaimable space" description="Unused SQLite pages that can be reclaimed by compaction.">
        <span data-testid="database-reclaimable">{stats ? formatBytes(stats.reclaimableBytes) : "Loading…"}</span>
      </SettingRow>
      {compactionRecommended && !stats?.pendingCompaction ? (
        <p data-testid="compaction-recommended" className="text-sm rounded-[8px] px-3 py-2 bg-[var(--bg-elevated)] text-[var(--text-secondary)]">
          {formatBytes(stats?.reclaimableBytes ?? 0)} of this file is deleted data the database
          still holds. Schedule a compaction to hand that space back to your disk.
        </p>
      ) : null}
      {stats && !stats.headroomOk ? (
        <p data-testid="database-headroom-warning" role="status" className="text-sm text-[var(--text-secondary)]">
          There may not be enough free disk space to compact right now. Compaction needs roughly the
          compacted database size ({formatBytes(Math.max(0, stats.databaseBytes - stats.reclaimableBytes))}) plus a small margin.
        </p>
      ) : null}
      {lastCompaction ? (
        <SettingRow id="database-last-compaction" label="Last compaction" description="Outcome of the most recent compaction attempt.">
          <span data-testid="database-last-compaction" className="text-sm text-[var(--text-secondary)]">{lastCompaction}</span>
        </SettingRow>
      ) : null}
      <SettingRow id="database-compact" label="Compact database" description={stats?.pendingCompaction ? "Compaction is scheduled for the next launch." : "On the next launch, RalphX compacts into a new database file, verifies it, then swaps it in and keeps the original as the backup."}>
        {stats?.pendingCompaction ? <Button variant="outline" disabled={saving} onClick={() => void setPending(false)}>Cancel scheduled compaction</Button> : <Button disabled={saving || !stats} onClick={() => setConfirming(true)} {...(saving && { "aria-label": "Scheduling compaction", "aria-busy": true })}>{saving ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" /> : "Compact on next launch"}</Button>}
      </SettingRow>
    </SettingsSection>
    <AlertDialog open={confirming} onOpenChange={setConfirming}>
      <AlertDialogContent><AlertDialogHeader><AlertDialogTitle>Compact the database on next launch?</AlertDialogTitle><AlertDialogDescription>On the next launch, before opening its database, RalphX compacts into a new file, verifies that file, then swaps it in — keeping the original as the backup. On a large database this takes several minutes, and RalphX shows the progress while it runs.</AlertDialogDescription></AlertDialogHeader><AlertDialogFooter><AlertDialogCancel>Cancel</AlertDialogCancel><AlertDialogAction onClick={(event) => { event.preventDefault(); void setPending(true); }}>Schedule compaction</AlertDialogAction></AlertDialogFooter></AlertDialogContent>
    </AlertDialog>
  </>;
}
