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

export interface RemoteErrorBannerProps {
  error: unknown;
  className?: string;
  testId?: string;
}

export function RemoteErrorBanner({
  error,
  className,
  testId,
}: RemoteErrorBannerProps) {
  const props = remoteErrorBannerProps(error);
  if (props === null) {
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
