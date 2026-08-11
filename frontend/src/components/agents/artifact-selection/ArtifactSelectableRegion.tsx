import {
  type HTMLAttributes,
  type PropsWithChildren,
  useContext,
  useLayoutEffect,
  useMemo,
  useRef,
} from "react";

import { ArtifactSelectionRegistrationContext } from "./ArtifactSelectionContext";
import type { ArtifactExcerptSource } from "./artifactSelection.types";

export interface ArtifactSelectableRegionProps
  extends PropsWithChildren,
    Omit<HTMLAttributes<HTMLDivElement>, "children"> {
  source: ArtifactExcerptSource;
}

export function ArtifactSelectableRegion({
  source,
  children,
  ...props
}: ArtifactSelectableRegionProps) {
  const registration = useContext(ArtifactSelectionRegistrationContext);
  const elementRef = useRef<HTMLDivElement>(null);
  const {
    artifactId,
    filePath,
    locator,
    revision,
    sessionId,
    sourceId,
    sourceKind,
    sourceLabel,
    title,
    url,
    version,
  } = source;
  const stableSource = useMemo(
    (): ArtifactExcerptSource => ({
      sourceKind,
      sourceId,
      sourceLabel,
      ...(artifactId ? { artifactId } : {}),
      ...(filePath ? { filePath } : {}),
      ...(locator ? { locator } : {}),
      ...(revision ? { revision } : {}),
      ...(sessionId ? { sessionId } : {}),
      ...(title ? { title } : {}),
      ...(url ? { url } : {}),
      ...(version !== undefined ? { version } : {}),
    }),
    [
      artifactId,
      filePath,
      locator,
      revision,
      sessionId,
      sourceId,
      sourceKind,
      sourceLabel,
      title,
      url,
      version,
    ],
  );

  useLayoutEffect(() => {
    const element = elementRef.current;
    if (!registration || !element) return;
    return registration.register(element, stableSource);
  }, [registration, stableSource]);

  return (
    <div ref={elementRef} data-artifact-selectable-region="true" {...props}>
      {children}
    </div>
  );
}
