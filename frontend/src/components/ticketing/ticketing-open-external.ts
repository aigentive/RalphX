import { openExternalUrl } from "@/lib/open-external";

export async function openExternalTicketUrl(url: string): Promise<void> {
  await openExternalUrl(url);
}
