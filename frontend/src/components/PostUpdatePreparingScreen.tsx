export function PostUpdatePreparingScreen() {
  return (
    <div
      className="flex h-full min-h-0 flex-1 flex-col"
      data-testid="post-update-preparing"
      style={{
        backgroundColor: "var(--app-content-bg)",
        color: "var(--text-primary)",
      }}
    >
      <div
        className="h-12 flex-shrink-0 border-b"
        data-tauri-drag-region
        style={{
          backgroundColor: "var(--app-header-bg)",
          borderBottomColor: "var(--app-header-border)",
          borderBottomStyle: "solid",
          borderBottomWidth: "1px",
        }}
      />
      <div className="flex flex-1 items-center justify-center px-8">
        <div className="flex max-w-[360px] flex-col items-center gap-4 text-center">
          <div
            aria-hidden="true"
            className="h-8 w-8 animate-spin rounded-full border-2 border-transparent"
            style={{
              borderTopColor: "var(--accent-primary)",
              borderRightColor: "var(--accent-primary)",
            }}
          />
          <div className="space-y-1">
            <h1 className="text-base font-semibold">Preparing RalphX</h1>
            <p className="text-sm" style={{ color: "var(--text-secondary)" }}>
              Finalizing the update and restoring the window.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
