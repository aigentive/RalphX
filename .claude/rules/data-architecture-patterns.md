---
paths:
  - "frontend/src/hooks/**"
  - "frontend/src/stores/**"
  - "frontend/src/api/**"
  - "src-tauri/src/commands/**"
  - "src-tauri/src/infrastructure/**/*_repo.rs"
---

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# Data Architecture Patterns

Universal code quality patterns for production-grade data loading and state management. Apply to any feature with server state and real-time updates.

## Core Patterns

| # | Pattern | Rule | Anti-Pattern |
|---|---------|------|--------------|
| 1 | **View-Driven Data Loading** | Each view requests only what it needs. Ask: "What does THIS view need?" not "What data exists?" | Hydrating entire domain objects with nested relations when view needs summary list only |
| 2 | **Query-First State Ownership** | Server state → React Query. UI state → Zustand/local store. If from server, Query owns it. | Syncing server data into Zustand and maintaining both manually |
| 3 | **Bounded Collections** | Never return unbounded arrays. If collection can grow >100 items, pagination required from day one. | `get_all_X()` endpoints returning entire tables as arrays |
| 4 | **Patch-First Cache Consistency** | Event occurs → patch query cache from payload. Refetch only if event lacks data. Priority: `setQueryData` > targeted `invalidateQueries` > broad prefix invalidation. | Invalidating entire query key prefixes on every mutation (refetch storms) |
| 5 | **Separate Read/Write Shapes** | Write commands return minimal confirmation. Read queries return rich view-specific data. Don't overload mutations with full updated entities. | Mutation endpoints returning "new state" that conflicts with query cache logic |
| 6 | **Explicit Resource Budgets** | Every paginated endpoint needs: default size, max size, backend clamping. Define and enforce limits. | Trusting frontend to "be reasonable" with page size parameters |
| 7 | **Migration Strategy** | **Pre-v1 (current):** Ask user before adding feature flags—we're still in active development. **Post-v1:** Big refactors go: add new → migrate behind flag → stable period → remove old. | Big-bang replacements forcing all-or-nothing migrations |
| 8 | **Polling as Last Resort** | Prefer event-driven updates. Polling only for recovery fallback with strict TTL. If adding polling: add stop condition, backoff strategy, max duration. | Multiple overlapping 2s polling intervals that never stop |
| 9 | **Index Before Querying** | Add DB indexes for new query patterns BEFORE production traffic. If query has `ORDER BY`/`WHERE` on new columns, add composite indexes first. | Shipping pagination queries without indexes, discovering N+1/table scans in prod |
| 10 | **Measure Before and After** | Establish performance baseline BEFORE refactoring. Measure improvement AFTER. Track: query count, payload bytes, render time, memory. | Claiming "it's faster" without metrics to prove it |

## Pagination Pattern

| Aspect | Specification |
|--------|---------------|
| Backend | Cursor-based pagination with `(cursor, limit, direction)`. Return `CursorPage<T>` with `items`, `next_cursor`, `has_more`. |
| Frontend | Use `useInfiniteQuery` for scroll-loading. Initial load = latest page, older pages on demand. |
| Defaults | Page size: 50 default, 200 max. Clamp on backend. |
| Ordering | Deterministic sort (e.g., `created_at DESC, id DESC`) to prevent duplicates across pages. |

## React Hooks: Multi-Effect State Machines

Complex hooks managing multiple interdependent effects should use **separate effects for separate concerns**, each tracking its own dimension via refs:

| Concern | Tracking Ref | Dependencies | Triggers |
|---------|--------------|--------------|----------|
| **Plan Change** | `prevSessionRef` | `ideationSessionId` | Wholesale state reset (clear user intent, recalculate auto-behavior) |
| **Count Transitions** | `prevCountsRef` | `taskCounts` | Detect 0→N changes; auto-expand unless user-blocked |
| **Initialization** | `initializedRef` | (gates other effects) | Skip spurious fires on mount; enable after init |
| **User Intent** | `userExpandedRef`, `userCollapsedRef` | (manual callbacks) | Track what user did to prevent auto-undo |

**Key pattern:** Session = plan scope; use for wholesale resets. Within same session, respect user choices.

**Example:** `useColumnCollapse` (`frontend/src/components/tasks/TaskBoard/useColumnCollapse.ts`)
- Effect 1 (`ideationSessionId`): Detect plan changes; auto-collapse empty on init/plan-change; preserve user-expanded within plan
- Effect 2 (`taskCounts`): Detect 0→N; auto-expand unless user-collapsed
- Callbacks: `toggleCollapse`, `expandColumn` track intent via refs

## External Store Subscriptions: useSyncExternalStore

For derived state computed from external store (e.g., React Query cache), use `useSyncExternalStore` with ref-based memoization:

```tsx
const subscribe = useCallback(
  (onStoreChange: () => void) => queryClient.getQueryCache().subscribe(onStoreChange),
  [queryClient],
);

const getSnapshot = useCallback((): Map<string, number> => {
  const next = new Map<string, number>();
  // Compute derived state from cache
  for (const col of columns) {
    const data = queryClient.getQueryData(cacheKey);
    // ... populate next
  }
  // Memoize: return prev ref if values unchanged (prevents Map instance churn)
  if (mapsEqual(prevRef.current, next)) return prevRef.current;
  prevRef.current = next;
  return next;
}, [...]);

return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
```

**Benefits:** Immediate reactivity to cache mutations (e.g., optimistic updates); no polling; prevents unnecessary parent re-renders via ref memoization.

**Example:** `useColumnTaskCounts` (`frontend/src/components/tasks/TaskBoard/useColumnTaskCounts.ts`) subscribes to queryClient cache, returns stable Map reference via ref-based equality.