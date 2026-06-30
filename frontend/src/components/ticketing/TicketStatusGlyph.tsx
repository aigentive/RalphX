import { Circle, CircleCheck, CircleDashed, CircleDot } from "lucide-react";

import type { TicketStateCategory } from "@/api/ticketing";

export function TicketStatusGlyph({
  category,
  className,
}: {
  category: TicketStateCategory;
  className: string;
}) {
  if (category === "done") {
    return <CircleCheck className={className} />;
  }
  if (category === "in_progress") {
    return <CircleDot className={className} />;
  }
  if (category === "other") {
    return <CircleDashed className={className} />;
  }
  return <Circle className={className} />;
}
