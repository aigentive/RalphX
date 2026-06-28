export const granolaDashboardKeys = {
  all: ["granola"] as const,
  settings: () => [...granolaDashboardKeys.all, "settings"] as const,
  notes: () => [...granolaDashboardKeys.all, "notes"] as const,
  noteDetail: (noteId: string | null) =>
    [...granolaDashboardKeys.notes(), "detail", noteId] as const,
};
