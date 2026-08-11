import { toast } from "sonner";

/**
 * Open an external URL through the Tauri opener. WKWebView does not reliably
 * handle target="_blank" itself, so user-facing external links should use this
 * seam instead of browser globals.
 */
export async function openExternalUrl(url: string): Promise<void> {
  if (!isHttpUrl(url)) {
    console.warn("Blocked external URL with unsupported scheme", { url });
    return;
  }

  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } catch {
    toast.error("Could not open the link in your browser.");
  }
}

function isHttpUrl(value: string): boolean {
  try {
    const parsed = new URL(value);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}
