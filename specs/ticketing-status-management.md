# Ticketing Status Management Spec

## Stage 0 Gate Baseline

This file is the authoritative Stage 0 baseline for ticketing status-management work. All Stage 1 implementation proposals for B1/B2/F1/F2 must treat this baseline as their prerequisite contract.

Plan reference: `fdc227c5-b394-4b18-854c-6f0ab9282df2` (`Ticketing Status Management - Operation Lifecycle Hardening (Fallback-Acceptance)`).

### Reconciliation Status

- At the time task `90b62284-ebcf-49c0-b80a-7bd9d6e3485f` was created, `specs/ticketing-status-management.md` was reported absent from the working tree and Git history.
- During Stage 0 execution, this path existed in the worktree and Git history as a status catalog/presentation spec added by `ba9c182d3` (`chore: auto-commit before freshness check`).
- The supplied content below is accepted as the real spec for status catalog, presentation, ordering, color, visibility, stale handling, and provider scope semantics.
- The supplied content does not define the operation-lifecycle hardening contract needed by the planned B1/B2/F1/F2 implementation tasks. For that missing operation-lifecycle scope, Stage 0 formally accepts Option 2 fallback: use the observed-code interpretation below until a more specific real spec is supplied.

### Observed-Code Operation Lifecycle Interpretation

Stage 1 status-management hardening is scoped to existing ticket mutation lifecycle behavior:

- Backend operation lifecycle integrity: `begin_operation` must claim a provider ticket operation atomically and `finish_operation` must fail closed when a terminal update cannot find the row.
- Scoped idempotency: duplicate handling must be scoped by provider, external ticket identity, operation kind, and target metadata; a bare `client_operation_id` match is not enough to prove the same user intent.
- Provider capabilities and mutation mechanics remain provider-authored: Jira uses transition ids, Linear uses state ids, ClickUp uses status names or provider-required status tokens.
- Operation events are part of the contract: `ticketing:operation_updated` is emitted for operation lifecycle changes and frontend state must consume it for reconciliation.
- List, kanban, and detail views may use optimistic state, but optimistic state is provisional until backend operation state and query invalidation reconcile it.
- Transition discovery failures, mutation failures, and missing valid transitions must surface scoped feedback and roll back optimistic state instead of silently disappearing.
- Operation history bootstrap must use the repository/service list path for provider ticket operations, exposed through an IPC command, rather than a new ad-hoc query path.

### Additive Provider Constraint

Stage 1 must be additive to the current Jira/Linear/ClickUp write model. Do not rewrite provider mutation APIs, transition payload semantics, or assignment/comment/label mutation mechanics while hardening lifecycle behavior.

Provider write semantics stay:

- Jira status transition: submit provider transition id.
- Linear status transition: submit Linear state id.
- ClickUp status transition: submit status name or provider-required status token.
- Assignment, comment, and label mutations keep their current provider-specific payload requirements.

### Stage 1 Constraints

1. Atomic claim belongs at the repository/service boundary and must be implemented consistently for SQLite and memory repositories.
2. Idempotency is scoped by provider, external ticket identity (`external_kind`, `external_id`, `external_key`), operation kind, and target metadata. Never treat bare `client_operation_id` as sufficient for cross-ticket or cross-target success.
3. `finish_operation` must fail closed when `update_provider_ticket_operation_status` returns `None`; it must not emit a success event or return the stale begun operation.
4. Missing write-command ids must generate unique fallback operation ids, or the command layer must require explicit ids. Do not introduce deterministic ids that collide across failed terminal retries.
5. Operation bootstrap must go through the existing provider-ticket-operation repository/service list path.
6. Frontend operation schemas must stay aligned with Rust `ProviderTicketOperationKind`, including `transition`, `assign`, `comment`, and `set_labels`.
7. UI feedback must preserve accessible status controls, disabled reasons, tooltip behavior, first-paint safety, and WKWebView-safe themed surfaces.
8. Status catalog/presentation work in the supplied real spec remains valid, but it is not a license to broaden Stage 1 into a full status-management UI unless a later task explicitly scopes that work.

### Avoid

1. Do not block Stage 1 on the historical absence of this spec; this file now records the accepted baseline.
2. Do not replace per-ticket provider transition legality with catalog ordering or local presentation state.
3. Do not treat reused `client_operation_id` values on different tickets, operations, or targets as idempotent success.
4. Do not swallow transition discovery failures, mutation failures, or missing-transition no-ops.
5. Do not collapse operation-bootstrap repository errors into "no operations" or misleading success UI.
6. Do not reuse or renumber existing migrations; any required schema change must be a new forward-only migration.
7. Do not build a large operation center UI for this hardening slice; reuse existing list, kanban, detail, and compact status surfaces unless verification expands scope.

### Proof Obligations

1. Atomic claim: tests prove two concurrent `begin_operation` calls for the same scoped intent create exactly one provider-bound pending operation and avoid a second provider call in both SQLite and memory repositories.
2. Fail-closed terminal update: tests prove `finish_operation` returns an error when the terminal row is missing and does not emit a succeeded event from stale state.
3. Scoped idempotency: tests cover pending/in-flight conflict, succeeded idempotent success for the same scoped intent, failed/timed-out/canceled requiring a new id, and different ticket/target never becoming false success.
4. Crash-window behavior: tests pin the chosen retry behavior when provider success occurs before `Succeeded` is persisted.
5. Schema/kind alignment: tests prove frontend operation responses parse all Rust operation kinds, including `set_labels`.
6. Optimistic rollback and failure surfacing: tests prove move and quick-assign failures surface scoped feedback and roll back list/kanban/detail state.
7. Event reconciliation: tests prove `ticketing:operation_updated` invalidates or reconciles the correct frontend queries and operation state.

### Downstream Gate Map

- B1: Backend operation-lifecycle integrity depends on the atomic claim, scoped idempotency, and fail-closed `finish_operation` contract above.
- B2: Backend operation bootstrap depends on the repository/service list-path and schema/kind-alignment contract above.
- F1: Frontend failure surfacing and optimistic rollback depends on the no-swallowed-failures and scoped feedback contract above.
- F2: Frontend operation bootstrap and `ticketing:operation_updated` handling depends on the event reconciliation contract above.

## Summary

RalphX needs first-class ticketing status management so imported provider statuses can be shown in a stable order, colored consistently, and edited for presentation without drifting from Linear, Jira, or ClickUp workflow reality.

The model is:

```text
provider status identity + observed provider metadata + RalphX presentation overrides = resolved status presentation
```

Providers remain authoritative for ticket state and valid transitions. RalphX owns presentation: display order, color override, visibility, stale handling, and accessible rendering.

## Goals

- Show statuses in a deterministic, user-controlled order even when provider APIs do not return an order.
- Color statuses consistently across list, kanban, detail, transition menus, and filters.
- Preserve sync with imported provider statuses across refreshes, renames, deletes, and provider color/category changes.
- Keep provider transition legality intact; ordering a status in RalphX must never imply that every ticket can transition to it.
- Support provider-specific status scopes without pretending all providers share a workflow model.
- Provide a status-management UI that is clear, reversible, and safe.

## Non-Goals

- Do not create custom provider workflows from RalphX.
- Do not map Linear/Jira/ClickUp statuses into one global cross-provider workflow in v1.
- Do not delete provider statuses locally.
- Do not replace per-ticket provider transition checks.
- Do not require every provider to expose colors or order.

## Current State

The frontend already models status-like columns:

```ts
TicketingColumn {
  id: string;
  name: string;
  category: "todo" | "in_progress" | "done" | "other";
  order: number;
  color?: string | null;
}
```

Ticket summaries also carry state:

```ts
TicketingState {
  id: string;
  name: string;
  category: "todo" | "in_progress" | "done" | "other";
  color?: string | null;
}
```

Current backend behavior:

- Linear workflow states are sorted by provider-like type and position.
- ClickUp statuses are sorted by `orderindex`.
- Jira project statuses are sorted by category because the current endpoint has no stable order field.
- Kanban/list grouping merges provider columns, ticket-derived columns, and transition-derived columns.
- Status icon rendering mostly uses category colors rather than provider/custom status colors.
- Optimistic transitions preserve the old ticket color, which can show the wrong color until refetch.

## Status Ownership

Provider-owned:

- Status identity.
- Ticket's current state.
- Valid transitions for a specific ticket.
- Provider status name, category, raw color, and raw order when available.
- Transition mutation payload requirements.

RalphX-owned:

- Display order.
- Color override.
- Visibility preference.
- Stale/deleted presentation.
- Theme-safe rendered color.
- UI grouping and picker ordering.

## Provider Scope Model

Status configuration must be scoped because providers do not agree on workflow ownership.

| Provider | Status Scope | Notes |
|---|---|---|
| Linear | Team | Workflow states are team-scoped; project filtering is not enough. |
| Jira | Project/workflow context | Statuses and transitions can vary by project and issue workflow. |
| ClickUp | Space | Statuses are space-scoped; tickets may be viewed by space, folder, or list. |

V1 should store status presentation by:

```text
provider + scope_kind + scope_id + provider_status_id
```

The UI may enter ticketing through a project/list/folder, but backend status resolution must derive the actual status scope used by that provider.

## Data Model

Add `ticketing_status_catalog`.

```text
id                           text primary key
provider                     text not null
scope_kind                   text not null
scope_id                     text not null
provider_status_id           text not null
provider_status_name         text not null
provider_category            text not null
provider_color               text null
provider_order               integer null
display_order                integer not null
color_override               text null
is_visible                   boolean not null default true
is_terminal                  boolean not null default false
last_seen_at                 text null
stale_since                  text null
created_at                   text not null
updated_at                   text not null
```

Unique index:

```sql
unique(provider, scope_kind, scope_id, provider_status_id)
```

Add optional provider metadata as JSON only if provider-specific gaps require it:

```text
provider_metadata_json       text null
```

Do not store transition ids in the catalog as primary status identity. Transition identity is per-ticket/per-workflow and belongs to transition-option responses.

## Resolved Status Contract

Backend should expose resolved columns:

```ts
interface TicketingColumn {
  id: string;
  name: string;
  category: TicketStateCategory;
  order: number;
  color?: string | null;
  providerColor?: string | null;
  colorOverride?: string | null;
  providerOrder?: number | null;
  scopeKind?: string;
  scopeId?: string;
  isVisible?: boolean;
  stale?: boolean;
  lastSeenAt?: string | null;
  staleSince?: string | null;
}
```

Resolved color:

```text
theme_safe(color_override ?? provider_color ?? category_fallback)
```

Resolved order:

```text
display_order, then provider_order, then category rank, then provider_status_name
```

Once `display_order` exists, provider category/order changes must not silently reshuffle user-managed statuses.

## Sync Algorithm

On status refresh for a provider scope:

1. Fetch provider statuses for the resolved scope.
2. Normalize each provider status to:

```text
provider_status_id
provider_status_name
provider_category
provider_color
provider_order
is_terminal
```

3. Upsert by unique provider/scope/status key.
4. Update observed provider fields.
5. Preserve RalphX fields: `display_order`, `color_override`, `is_visible`.
6. Assign `display_order` to new statuses after the largest existing display order in the scope.
7. Set `last_seen_at` for statuses returned by the provider.
8. For catalog rows missing from the provider response, set `stale_since` if not already set.
9. Do not delete stale statuses automatically.
10. Keep stale statuses renderable when tickets still reference them.

Two-phase stale handling:

- Stale + still referenced by tickets: render in list/detail/kanban, show stale badge in management UI.
- Stale + unreferenced + no valid transitions: hide from move picker by default, keep manageable/restorable.

## Transition Semantics

Status catalog order must never replace per-ticket transition legality.

For a ticket move menu:

```text
ordered resolved catalog statuses intersected with live transition options for this ticket
```

Provider-specific mutation behavior remains:

- Linear: submit issue state id.
- Jira: submit provider transition id.
- ClickUp: submit status name or provider-required status token.

Keep these concepts separate:

```text
status identity: provider_status_id
transition identity: provider_transition_id or provider-specific mutation payload
```

Transition options should carry display color/order through matching resolved catalog rows when possible.

## API Changes

Add commands:

```text
list_ticketing_status_catalog(provider, scopeKind, scopeId)
update_ticketing_status_presentation(provider, scopeKind, scopeId, patches)
refresh_ticketing_status_catalog(provider, scopeKind, scopeId)
```

Patch shape:

```ts
interface TicketingStatusPresentationPatch {
  providerStatusId: string;
  displayOrder?: number;
  colorOverride?: string | null;
  isVisible?: boolean;
}
```

Batch update rules:

- Reorder is atomic per scope.
- Partial reorder preserves relative order for statuses not included in the patch.
- Color reset sends `colorOverride: null`.
- Visibility changes never delete rows.

Existing `list_ticketing_columns` should return resolved catalog columns once the catalog is available. During migration, it may fall back to live provider-derived columns and seed the catalog opportunistically.

## UX

### Status Management Surface

Entry points:

- Ticketing dashboard toolbar.
- Provider settings section.
- Empty/stale-status warning action.

Primary controls:

- Provider selector.
- Scope selector:
  - Linear team.
  - Jira project/workflow context.
  - ClickUp space.
- Ordered status list with drag handles.
- Color swatch button.
- Visibility toggle.
- Reset controls.

Each row should show:

- Status name.
- Category glyph.
- Current resolved color.
- Provider color indicator when different from override.
- Badges: imported, custom color, hidden, stale.
- Provider/scope context when names are ambiguous.

Reset actions:

- Reset color.
- Reset order.
- Reset visibility.
- Reset all presentation for this scope.

### Dashboard Behavior

List and kanban:

- Use resolved catalog order.
- Use resolved color for icons/dots/column accents.
- Hidden statuses do not create empty columns.
- Hidden statuses still appear when tickets currently use them.
- Stale statuses render with a subtle stale indicator in detail/management surfaces, not noisy warning text in every row.

Transition menu:

- Statuses appear in resolved catalog order.
- Only valid per-ticket transitions are enabled.
- Disabled provider transitions show the provider disabled reason.
- Hidden statuses can still appear if the current ticket can transition to them, but should be visually de-emphasized or marked hidden.

Color UX:

- Provide preset swatches first.
- Allow custom hex only if validation and contrast handling exist.
- Offer reset-to-provider/default.
- High-contrast theme may override custom colors for accessibility while preserving the saved override.

## Accessibility and Theming

Raw provider colors are data, not final UI tokens.

Rendering must pass through a color resolver that:

- Validates color syntax.
- Falls back on invalid colors.
- Computes accessible foreground/background variants.
- Handles light, dark, and high-contrast themes.
- Avoids relying on color alone; status name/glyph remains visible.

For WKWebView safety, themed surfaces should use explicit background/border longhands and avoid chained CSS var assumptions.

## Edge Cases

| Case | Expected Behavior |
|---|---|
| Provider returns no order | Seed by category/name, then persist RalphX `display_order`. |
| Provider changes order | Preserve RalphX order unless user resets order. |
| Provider renames status | Update name, keep overrides. |
| Provider changes color | Update provider color, keep override. |
| Provider deletes status | Mark stale; keep renderable if referenced. |
| Duplicate status names | Show scope/provider context in management UI. |
| Status appears in tickets but not columns | Create ticket-derived fallback row marked observed-from-ticket. |
| Ticket transitions unavailable | Show status as current/read-only; omit or disable move action. |
| Jira same target status with different transition ids | Resolve transition id from live ticket transitions, not catalog. |
| ClickUp status id missing | Use stable normalized status name for identity, with metadata noting name-based identity. |
| Invalid provider color | Store raw value only if useful for diagnostics; render fallback color. |
| Offline/stale provider | Use cached catalog for display; block writes that require live transition validation. |

## Migration Strategy

1. Add catalog table and repository.
2. Add provider status normalization helpers.
3. Seed catalog from existing `list_ticketing_columns` calls.
4. Switch `list_ticketing_columns` to return resolved catalog columns.
5. Add presentation update command.
6. Add frontend resolved-color/order rendering.
7. Add status management UI.
8. Fix optimistic transition color to use target column/catalog color.

No destructive migration is needed. Existing ticket data remains provider-authored.

## Validation

Backend tests:

- Sync inserts new statuses with assigned display order.
- Sync preserves color/order/visibility overrides.
- Provider rename updates name without losing overrides.
- Provider deletion marks stale instead of deleting.
- New provider statuses append after existing custom order.
- Reorder patch is atomic and preserves untouched relative order.
- Transition menu construction still requires live transition options.

Frontend tests:

- List and kanban group by resolved order.
- Status icon uses resolved color.
- Invalid/low-contrast colors fall back safely.
- Hidden statuses are excluded from empty columns.
- Hidden/stale statuses still render when tickets use them.
- Optimistic transition uses target status color.
- Duplicate names render enough context in management UI.

Visual tests:

- Ticketing list light/dark/high-contrast.
- Ticketing kanban light/dark/high-contrast.
- Status management dialog with imported/custom/hidden/stale statuses.

## Implementation Notes

- Keep status presentation logic in pure helpers where possible.
- Do not add provider-specific branching in React components when backend can return resolved presentation.
- Keep `TicketingColumn` backward-compatible during rollout by making new fields optional.
- Do not make status management a blocking dependency for opening the dashboard; use cached/fallback columns and invalidate when sync completes.
- Keep provider write paths authoritative and idempotent through existing ticket operation tracking.

## Open Questions

- Should Linear expose team selection directly, or should RalphX infer team from selected project/ticket?
- Should Jira v1 stay project-scoped, or should it fetch board/workflow contexts where available?
- Should color overrides be per provider scope only, or should RalphX later support reusable color templates?
- Should hidden statuses appear in move menus when valid, or only when currently selected?
- How aggressively should stale statuses be surfaced to users outside the management UI?
