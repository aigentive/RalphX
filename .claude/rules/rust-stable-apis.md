---
paths:
  - "src-tauri/**/*.rs"
  - "src-tauri/Cargo.toml"
  - "rust-toolchain.toml"
---

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# Rust Stable API Safety

## Goal

Prevent compile breaks from unstable std APIs (e.g., `unsigned_is_multiple_of`) on non-nightly toolchains.

## Rules (NON-NEGOTIABLE)

- Default to stable Rust APIs only. ❌ Unstable std methods/features unless user explicitly asks for nightly-only work.
- Divisibility: `u{N}::is_multiple_of` is stable since Rust 1.87, but project policy keeps the `%` form for portability to older stable toolchains. ✅ `x % n == 0` (with zero guard where needed) | ❌ converting to `x.is_multiple_of(n)`. Clippy's `manual_is_multiple_of` (warned via Cargo.toml `[lints.clippy]`) will suggest the conversion — suppress per site with `#[allow(unknown_lints, clippy::manual_is_multiple_of)]` (as in chat_service_streaming.rs); do NOT convert.
- If unstable API is truly required, gate it explicitly and document nightly requirement in PR/commit notes.
- Treat `rust-toolchain.toml` as source of truth for project toolchain expectations.

```rust
// ✅ Stable and portable
if interval != 0 && current_iteration > 0 && current_iteration % interval == 0 {
    // checkpoint
}

// ❌ Do not use (project policy: keep % form; allow the clippy lint per site)
if current_iteration.is_multiple_of(interval) { /* ... */ }
```
