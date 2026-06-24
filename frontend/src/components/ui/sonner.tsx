import {
  CircleCheckIcon,
  InfoIcon,
  Loader2Icon,
  OctagonXIcon,
  TriangleAlertIcon,
} from "lucide-react"
import { useTheme } from "next-themes"
import { Toaster as Sonner, type ToasterProps } from "sonner"

const Toaster = ({ style, ...props }: ToasterProps) => {
  const { theme = "system" } = useTheme()
  const effectiveTheme: "light" | "dark" | "system" = theme === "light" || theme === "dark" || theme === "system" ? theme : "system"

  return (
    <Sonner
      theme={effectiveTheme}
      className="toaster group"
      position="bottom-left"
      icons={{
        success: <CircleCheckIcon className="size-4" />,
        info: <InfoIcon className="size-4" />,
        warning: <TriangleAlertIcon className="size-4" />,
        error: <OctagonXIcon className="size-4" />,
        loading: <Loader2Icon className="size-4 animate-spin" />,
      }}
      style={
        {
          "--normal-bg": "var(--popover)",
          "--normal-text": "var(--popover-foreground)",
          "--normal-border": "var(--border)",
          "--border-radius": "var(--radius)",
          zIndex: 40,
          ...style,
        } as React.CSSProperties
      }
      {...props}
    />
  )
}

export { Toaster }
