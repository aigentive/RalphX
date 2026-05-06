/**
 * TaskBoardSkeleton - Loading placeholder for the task board
 *
 * Design: v29a Kanban — stable full-height columns with 1px dividers.
 */

const COLUMN_COUNT = 5;

export function TaskBoardSkeleton() {
  return (
    <div
      data-testid="task-board-skeleton"
      className="grid flex-1 overflow-x-auto"
      style={{
        gridTemplateColumns: `repeat(${COLUMN_COUNT}, minmax(220px, 1fr))`,
        gap: "1px",
        background: "var(--kanban-board-divider)",
      }}
    >
      {Array.from({ length: COLUMN_COUNT }).map((_, index) => {
        return (
          <div
            key={index}
            data-testid={`skeleton-column-${index}`}
            className="flex min-w-[220px] flex-col"
            style={{ background: "var(--kanban-column-bg)" }}
          >
            {/* Column header - simple */}
            <div
              data-testid={`skeleton-header-${index}`}
              className="flex items-center gap-2"
              style={{ padding: "14px 12px 10px" }}
            >
              <div
                className="h-2.5 flex-1 rounded animate-pulse"
                style={{ background: "var(--overlay-weak)", maxWidth: "70px" }}
              />
              <div
                className="h-2.5 w-4 rounded animate-pulse"
                style={{ background: "var(--bg-elevated)" }}
              />
            </div>

            {/* Drop zone */}
            <div className="flex-1 space-y-2" style={{ padding: "4px 12px 16px" }}>
              {/* Card placeholders */}
              {Array.from({ length: (index % 3) + 1 }).map((_, cardIndex) => (
                <div
                  key={cardIndex}
                  data-testid={`skeleton-card-${index}-${cardIndex}`}
                  className="p-2.5 rounded-lg animate-pulse"
                  style={{
                    background: "var(--kanban-card-bg)",
                    border: "1px solid var(--kanban-card-border)",
                  }}
                >
                  <div
                    className="h-3 w-4/5 rounded mb-2"
                    style={{ background: "var(--bg-elevated)" }}
                  />
                  <div
                    className="h-2.5 w-3/5 rounded"
                    style={{ background: "var(--overlay-weak)" }}
                  />
                </div>
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}
