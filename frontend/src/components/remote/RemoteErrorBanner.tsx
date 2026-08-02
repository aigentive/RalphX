/**
 * Renders the 2.6 `remoteErrorBannerProps` mapping wherever a gated action can still
 * fail after the click (PR 2.7 placement).
 *
 * 2.6 shipped the mapper and its tests but no call site, so a scope that narrowed
 * mid-flight produced a rejected promise the composer swallowed and the permission
 * dialog reported as a generic "please retry" toast — advice that is wrong for both
 * codes it covers, because retrying a `REMOTE_FORBIDDEN` or a
 * `REMOTE_COMMAND_UNAVAILABLE` cannot succeed.
 *
 * Returning `null` for every other error is the mapper's own contract: this component
 * adds a surface, never a new classification.
 */

import { AlertTriangle } from "lucide-react";

import { NoticeBanner } from "@/components/ui/notice-banner";
import { remoteErrorBannerProps } from "@/lib/remote/agent-gate";
import { isRemoteTransportError } from "@/lib/remote/transport-errors";

export interface RemoteErrorBannerProps {
  error: unknown;
  className?: string;
  testId?: string;
  /**
   * Render a generic connection-failure banner for transport errors whose code has no
   * mapped copy. Default OFF: unmapped codes render nothing so surfaces with their own
   * treatment are not double-bannered. Opt in only when this banner is the surface's
   * ONLY error presentation (e.g. PermissionDialog's kept-queued resolve failure).
   */
  fallbackForTransportErrors?: boolean;
}

export function RemoteErrorBanner({
  error,
  className,
  testId,
  fallbackForTransportErrors = false,
}: RemoteErrorBannerProps) {
  const props = remoteErrorBannerProps(error);
  if (props === null) {
    if (fallbackForTransportErrors && isRemoteTransportError(error)) {
      return (
        <NoticeBanner
          tone="error"
          icon={<AlertTriangle size={14} aria-hidden="true" />}
          title="Remote connection failed"
          testId={testId ?? "remote-error-banner"}
          {...(className === undefined ? {} : { className })}
        >
          {error.message}
        </NoticeBanner>
      );
    }
    return null;
  }
  return (
    <NoticeBanner
      tone={props.tone}
      icon={<AlertTriangle size={14} aria-hidden="true" />}
      title={props.title}
      testId={testId ?? "remote-error-banner"}
      {...(className === undefined ? {} : { className })}
    >
      {props.body}
    </NoticeBanner>
  );
}
