export const granolaDashboardKeys = {
  all: ["granola"] as const,
  settings: () => [...granolaDashboardKeys.all, "settings"] as const,
  notes: (projectId?: string | null) =>
    [...granolaDashboardKeys.all, "notes", projectId ?? "global"] as const,
  noteDetail: (noteId: string | null) =>
    [...granolaDashboardKeys.notes(), "detail", noteId] as const,
};
