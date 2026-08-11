import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { databaseMaintenanceApi, type DatabaseMaintenanceStats } from "@/api/database-maintenance";
import { SettingsSection, SettingRow } from "./SettingsView.shared";

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let size = value / 1024;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) { size /= 1024; unit += 1; }
  return `${size.toFixed(size >= 10 ? 0 : 1)} ${units[unit]}`;
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
  return <>
    <SettingsSection>
      {error ? <p role="alert" className="text-sm text-destructive">{error}</p> : null}
      <SettingRow id="database-size" label="Database size" description="Current local RalphX database footprint.">
        <span data-testid="database-size">{stats ? formatBytes(stats.databaseBytes) : "Loading…"}</span>
      </SettingRow>
      <SettingRow id="database-reclaimable" label="Estimated reclaimable space" description="Unused SQLite pages that can be reclaimed by compaction.">
        <span data-testid="database-reclaimable">{stats ? formatBytes(stats.reclaimableBytes) : "Loading…"}</span>
      </SettingRow>
      <SettingRow id="database-compact" label="Compact database" description={stats?.pendingCompaction ? "Compaction is scheduled for the next launch." : "Creates a verified backup, then compacts before the database opens on the next launch."}>
        {stats?.pendingCompaction ? <Button variant="outline" disabled={saving} onClick={() => void setPending(false)}>Cancel scheduled compaction</Button> : <Button disabled={saving || !stats} onClick={() => setConfirming(true)}>{saving ? <Loader2 className="h-4 w-4 animate-spin" /> : "Compact on next launch"}</Button>}
      </SettingRow>
    </SettingsSection>
    <AlertDialog open={confirming} onOpenChange={setConfirming}>
      <AlertDialogContent><AlertDialogHeader><AlertDialogTitle>Compact the database on next launch?</AlertDialogTitle><AlertDialogDescription>RalphX will wait until the next launch, before opening its database, verify a backup, and compact only when sufficient disk space is available.</AlertDialogDescription></AlertDialogHeader><AlertDialogFooter><AlertDialogCancel>Cancel</AlertDialogCancel><AlertDialogAction onClick={(event) => { event.preventDefault(); void setPending(true); }}>Schedule compaction</AlertDialogAction></AlertDialogFooter></AlertDialogContent>
    </AlertDialog>
  </>;
}
