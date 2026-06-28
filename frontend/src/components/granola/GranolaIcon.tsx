import type { CSSProperties, HTMLAttributes } from "react";

import granolaIconSvg from "@/assets/granola-ai-icon.svg";

type GranolaIconProps = HTMLAttributes<HTMLSpanElement> & {
  strokeWidth?: number | undefined;
};

export function GranolaIcon({
  style,
  strokeWidth: _strokeWidth,
  ...props
}: GranolaIconProps) {
  return (
    <span
      aria-hidden="true"
      style={{
        display: "inline-block",
        backgroundImage: `url(${granolaIconSvg})`,
        backgroundPosition: "center",
        backgroundRepeat: "no-repeat",
        backgroundSize: "contain",
        ...style,
      } as CSSProperties}
      {...props}
    />
  );
}
