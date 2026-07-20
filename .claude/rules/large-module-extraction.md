---
paths:
  - "src-tauri/src/**/*.rs"
  - "frontend/src/**/*.{ts,tsx}"
---

# Large Module Extraction Patterns

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

## When to Extract

| Trigger | Action | Target |
|---------|--------|--------|
| Module >500 lines | Extract functions to sub-modules | 350-450 lines each |
| >50 tests in one file | Move to `tests/` subdir with 5-10 test files | 100-300 lines each |
| >5 impl blocks | Split by domain (branch ops, query ops, etc.) | ~300 lines per module |
| >30 functions in impl | Break into logical groups | 8-15 functions/module |

## Parallel Extraction Strategy (Rust)

When extracting large single file into 10+ target files:

### Step 1: Analyze Source
1. Read entire source file once
2. Identify function groups by domain (branch ops, query ops, etc.)
3. Identify test boundaries and shared helpers
4. Plan target files + function assignments

### Step 2: Mechanical Extraction (NON-NEGOTIABLE)
1. Plan exact line ranges per target file from the Step 1 analysis
2. Move bodies with scripted extraction commands only — `sed -n 'A,Bp'` / `awk` / `git mv` — never by retyping or agent-recreating existing bodies (see code-quality-standards.md "Mechanical extraction only")
3. `apply_patch`/Edit is reserved for the post-move fix-up layer: imports, visibility, re-exports, module wiring
4. Parallelism is allowed for the READ/ANALYSIS phase only; the moves themselves run as one serial scripted pass

### Step 3: Main Thread Work
After all agents complete:
1. Rewrite mod.rs/index.ts to type hub + re-exports
2. Add module declarations for new sub-modules
3. Run the narrowest compile/test command covering the extracted module
4. Run touched-leaf formatting checks; leave broad CI parity to CI
5. Single commit (all files atomic)

## Visibility Fixes (Rust)

When extracting from single impl block into separate modules:

| Caller | Called Function | Visibility |
|--------|-----------------|-----------|
| Same module | Any | No change (leave private) ✅ |
| Different module | Private fn | `pub(super)` |
| Test in tests/ subdir | Private fn | `pub(super)` (can't see private) |
| External crate | — | Must be `pub` |

**Discovery:** Run the narrowest repo-appropriate compile/test command for the extracted module → note errors → grep call sites → apply visibility fix.

## Test Helper Extraction (Rust)

When moving tests to `tests/` subdir:

### Problem
- Helper fn `init_test_repo()` in `commit_tests.rs`
- Needed by `merge_tests.rs`
- Tests in separate files can't cross-access

### Solution
1. Move shared helper to `tests/mod.rs` as `pub fn helper() { ... }`
2. Declare submodules: `mod commit_tests; mod merge_tests;`
3. Import in test files: `use super::init_test_repo;`
4. If only one test uses helper → leave in that test file (private) ✅

### Example
```rust
// tests/mod.rs
pub fn init_test_repo() -> TempDir { ... }
mod branch_tests;
mod commit_tests;
mod merge_tests;

// tests/merge_tests.rs
use super::init_test_repo;

#[test]
fn test_merge() {
    let repo = init_test_repo();
    // ...
}
```

## Re-export Hub Pattern (Rust)

After extraction, mod.rs becomes type + re-export hub:

```rust
//! Module doc comment explaining the service

pub mod branch;
pub mod commit;
pub mod query;
pub mod state_query;
pub mod worktree;
pub mod tests;

// Re-export main types for external callers
pub use branch::BranchOps;
pub use commit::CommitInfo;
pub use state_query::MergeConflict;

// Service API entry point
pub struct GitService { ... }

impl GitService {
    // Only foundational methods that orchestrate sub-modules
    // Most logic delegated to sub-module impls
}
```

**Benefits:**
- Clear API surface
- Import chain unchanged (`use git_service::GitService`)
- Sub-modules remain private implementation details
- Easier to refactor internal organization

## TypeScript Module Extraction (Frontend)

Similar pattern for large TS modules:

1. Extract classes/functions to separate files (one per domain)
2. Create `index.ts` that re-exports main types
3. Use `export { Class as OldName }` if renaming
4. Run `npm run typecheck` before commit
5. Atomic commit with all new files

## Post-Extraction Cleanup

After extracting functions to sub-modules, orphaned code may remain in the parent module. Detect and remove it.

### Common Orphan Pattern

Functions deleted from parent but left between two `impl` closures. Stray attributes like `#[cfg(test)]` break the first `impl` closure prematurely. Result: orphaned functions without `impl` wrapper → compile error "unexpected closing delimiter `}`".

### Detection Strategy

| Step | Action |
|------|--------|
| 1 | Compare `fn` signatures in parent (`mod.rs`) vs extracted modules — find duplicates |
| 2 | Look for stray attributes (`#[cfg(test)]`) between `impl` closing braces |
| 3 | Look for unmatched `}` at unexpected positions |
| 4 | Run the targeted filter/suite for the extracted module per rust-test-execution.md — "unexpected closing delimiter" = orphaned block |

### Resolution

1. Identify exact line range (stray attribute start → orphaned `}` end)
2. Verify functions in block are fully defined in extracted modules (no partial moves)
3. Delete the entire range in one edit
4. Verify compile succeeds

### Prevention Checklist

| Step | Action | Verify |
|------|--------|--------|
| Extract module | Create new leaf file with functions | `pub fn` signatures match extracted body |
| Update parent | Delete functions from parent | Grep confirms no duplicates in parent |
| Verify structure | Check `impl` block boundaries | No stray attributes between `impl` closing `}` and next item |
| Compile | Run the targeted filter/suite for the extracted module per rust-test-execution.md | 0 compile errors, structure is clean |

## Verification Checklist

- All functions assigned to correct module
- `use super::*;` (Rust) or top-level imports (TS)
- Shared test helpers moved to `tests/mod.rs`
- Private functions with cross-module callers → `pub(super)`
- Module declarations added to parent `mod.rs`
- No orphaned duplicates in parent (see Post-Extraction Cleanup above)
- Focused tests/checks for the extracted module pass / `npm run typecheck` succeeds
- Touched Rust leaf files pass `rustfmt --edition 2021 --check`
- Compilation succeeds with no new warnings
- Single atomic commit
