---
paths:
  - "src-tauri/**/*.rs"
---

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# Tokio Runtime Safety

## Context

A past regression put `tokio::spawn` in the synchronous `ThrottledEmitter::new()` constructor, which panics at runtime because no Tokio reactor is set on the main thread during Tauri app setup. This passes `cargo check` and tests but crashes the app on launch. Current correct implementation: `src-tauri/src/application/throttled_emitter.rs` (std::thread::spawn).

## API Summary (NON-NEGOTIABLE)

| API | Context Required | When to Use |
|-----|-----------------|-------------|
| `tokio::spawn` / `tokio::task::spawn` | Tokio runtime on calling thread | Inside `async fn` only |
| `tokio::task::spawn_blocking` | Tokio runtime on calling thread | Inside `async fn` only |
| `tauri::async_runtime::spawn` | None (uses Tauri's managed handle) | Tauri `setup()` closure, sync code needing async |
| `std::thread::spawn` | None | Sync constructors, background loops, no runtime needed |

## Rules

- `tokio::spawn` / `tokio::task::spawn` / `tokio::task::spawn_blocking` → **async context ONLY**. Using in sync code → runtime panic on launch.
- Tauri `setup()` closure + other sync app init → `tauri::async_runtime::spawn()` (uses Tauri's managed runtime handle).
- Background loops in sync constructors (`fn new()`) → `std::thread::spawn` + `std::thread::sleep`. No Tokio dependency.

```rust
// ✅ Correct — sync constructor uses std::thread::spawn
impl ThrottledEmitter {
    pub fn new(sink: Arc<dyn EventSink>) -> Arc<Self> {
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(100));
            // ...
        });
    }
}

// ❌ Wrong — tokio::spawn in sync constructor panics on launch
impl ThrottledEmitter {
    pub fn new(sink: Arc<dyn EventSink>) -> Arc<Self> {
        tokio::spawn(async move { /* ... */ }); // PANIC: no reactor running
    }
}
```
