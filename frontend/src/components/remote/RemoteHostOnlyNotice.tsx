/**
 * The standard "this pane is answered by the host, not this Mac" banner.
 *
 * Several settings surfaces read commands the facade deliberately refuses — provider
 * settings (`Denied`: CLI probes, provider identities, credential surface), ticketing and
 * GitHub (`Elevated`: credential/network process authority), project creation (`Elevated`:
 * git-init at caller paths). Under a remote environment those reads never answer, and every
 * one of those panes rendered its LOADING state forever, which reads as "the app is broken"
 * rather than "this runs over there".
 *
 * A capability boundary is information, not a failure: the pane says which machine owns the
 * setting and names it, so the user knows where to go. Full-bleed under the pane heading
 * rather than an inline card, because it qualifies the whole surface beneath it.
 */

import { RadioTower } from "lucide-react";

import { NoticeBanner, type NoticeBannerLayout } from "@/components/ui/notice-banner";
import { useActiveEnvironment } from "@/hooks/useActiveEnvironment";

export interface RemoteHostOnlyNoticeProps {
  /** What the host owns, e.g. "Provider setup". Used as the banner title. */
  subject: string;
  /** One sentence on what to do instead. */
  detail?: string;
  testId?: string;
  /** `strip` sits flush under a dialog header; `card` is the default inline banner. */
  layout?: NoticeBannerLayout;
  className?: string;
}

/**
 * Renders the host-only banner for the ACTIVE environment.
 *
 * Callers gate on `useIsRemoteEnvironment()`; this component only formats the answer. It
 * degrades to the generic wording when the registry entry is not resolvable, which is the
 * same window `useActiveEnvironmentKind` fails closed over.
 */
export function RemoteHostOnlyNotice({
  subject,
  detail,
  testId = "remote-host-only-notice",
  layout = "card",
  className,
}: RemoteHostOnlyNoticeProps) {
  const environment = useActiveEnvironment();
  // The title carries the user's label for the host; the address is always shown beneath it.
  // A renamed environment ("Studio Mac") would otherwise leave the banner unable to say WHICH
  // machine, which is the one question it exists to answer.
  const name = environment?.name ?? "the remote host";
  // `remote` is optional on the entry even when `kind` is "remote", so narrow on the field
  // rather than the discriminant.
  const baseUrl = environment?.remote?.baseUrl;

  return (
    <NoticeBanner
      tone="warning"
      testId={testId}
      icon={<RadioTower className="h-4 w-4" aria-hidden="true" />}
      title={`${subject} runs on ${name}`}
      layout={layout}
      className={className ?? "mb-4 w-full"}
    >
      <span>
        {detail ??
          "This setting belongs to the machine you are connected to, so it is not editable from here."}
      </span>
      {baseUrl !== undefined && (
        <span className="mt-1 block font-mono text-xs text-[var(--text-muted)]">
          {baseUrl}
        </span>
      )}
    </NoticeBanner>
  );
}
