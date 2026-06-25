import { toast } from "sonner";

/**
 * Open an external URL through the Tauri opener (WKWebView does not reliably open
 * target="_blank" links itself). Surfaces a toast instead of an unhandled rejection
 * if the open fails, so a click that goes nowhere still gives the user feedback.
 */
export async function openExternalTicketUrl(url: string): Promise<void> {
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } catch {
    toast.error("Could not open the link in your browser.");
  }
}
