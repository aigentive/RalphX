import type { CSSProperties, HTMLAttributes } from "react";

import githubInvertocatSvg from "@/assets/github-invertocat.svg";

type GitHubMarkIconProps = HTMLAttributes<HTMLSpanElement> & {
  strokeWidth?: number | undefined;
};

export function GitHubMarkIcon({
  style,
  strokeWidth: _strokeWidth,
  ...props
}: GitHubMarkIconProps) {
  return (
    <span
      aria-hidden="true"
      style={{
        display: "inline-block",
        backgroundColor: "currentColor",
        WebkitMaskImage: `url(${githubInvertocatSvg})`,
        maskImage: `url(${githubInvertocatSvg})`,
        WebkitMaskRepeat: "no-repeat",
        maskRepeat: "no-repeat",
        WebkitMaskPosition: "center",
        maskPosition: "center",
        WebkitMaskSize: "contain",
        maskSize: "contain",
        ...style,
      } as CSSProperties}
      {...props}
    />
  );
}
