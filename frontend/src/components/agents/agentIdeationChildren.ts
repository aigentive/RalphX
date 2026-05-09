import type { IdeationSessionResponse } from "@/api/ideation.types";

export function getLatestIdeationChildId(
  children: IdeationSessionResponse[] | undefined,
): string | null {
  if (!children?.length) {
    return null;
  }
  const sorted = [...children].sort(
    (a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
  );
  return sorted[0]?.id ?? null;
}
