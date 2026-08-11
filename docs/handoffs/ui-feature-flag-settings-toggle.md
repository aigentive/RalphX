# Handoff: UI Feature-Flag Toggle in Settings (Automations first)

**Date:** 2026-07-06
**Author:** Spec pass (3-researcher ground-truth on the feature-flag + Settings architecture)
**Status:** Implementation-ready
**Scope owner:** whoever picks this up next — an implementer with no access to the originating conversation can build from this alone.

All `file:line` references were verified against the codebase on 2026-07-06. Items marked **NEW:** do not exist yet. Items marked **VERIFY:** are load-bearing assumptions to confirm before relying on them.

---

## 1. Objective

Let a user turn the **Automations** section on/off from the in-app **Settings** UI, instead of only via `config/ralphx.yaml` (`ui.feature_flags.automations_page`) or the `RALPHX_UI_AUTOMATIONS_PAGE` env var. Ship it as a general, extensible "Labs / Experimental" toggle surface so other UI flags can be exposed the same way later, but wire only the `automations_page` toggle in v1.

The config default was just set to `automations_page: true` (`config/ralphx.yaml:40`) — that stays as the **ship/default baseline**. This handoff adds a **per-user override** on top of that baseline, editable from Settings, that survives restart and (ideally) takes effect live.

### Non-goals (v1)
- Exposing every flag in Settings — only `automations_page`. The mechanism is generic; the UI wires one toggle.
- Changing backend behavior gating (there is none for this flag — see §2).
- Per-project feature flags (this is app-global, like the current flags).
- Operator "managed/locked by env" UX (deferred — see §7 and §12).

---

## 2. Current state (ground truth)

### 2.1 The flag is frozen at startup — a live toggle cannot mutate the cached config
- Rust type `UiFeatureFlagsConfig` — `src-tauri/src/infrastructure/agents/claude/agent_config/ui_config.rs:15-50`. 8 `bool` fields, `#[serde(default)]`, **snake_case** serde. `Default` sets `automations_page: true` (`ui_config.rs:37-50`).
- Precedence (low→high): struct `Default` → YAML `ui.feature_flags` (bound at `runtime_config/mod.rs:1162`) → env `RALPHX_UI_*` (`runtime_config.rs:1052-1078`; automations at `:1062-1065`). Env is last-write-wins and, when present, any value other than `true`/`1` = `false`.
- **Cache primitive:** `static LOADED_CONFIG_CELL: OnceLock<LoadedConfig>` — `runtime_config/mod.rs:368`, populated once via `.get_or_init(load_config)`. `ui_feature_flags_config()` (`mod.rs:1835-1840`) and `default_ui_feature_flags()` (`harness_runtime_registry.rs:780-782`) return clones of the cached `&'static`.
- **CONFIRMED immutable for the process lifetime.** No reset/reload path on stable Rust. Therefore any writable override MUST live **outside** `LOADED_CONFIG_CELL`; the cached config becomes the immutable **baseline** and the override is layered on top per-field.

### 2.2 The read command is a pure, argument-less read; there is no write path
- `get_ui_feature_flags` — `src-tauri/src/commands/ui_commands.rs:24-37`. `#[tauri::command] pub fn get_ui_feature_flags() -> UiFeatureFlagsResponse`. Takes **no input, no `app_state`, no `db`** — pure read of the cached config.
- Response `UiFeatureFlagsResponse` — `ui_commands.rs:9-20`, `#[serde(rename_all = "camelCase")]` → serializes `automationsPage` etc. (test asserts camelCase: `ui_commands_tests.rs:21-22,54-55`).
- Registration: `commands/mod.rs:356` → `register_tauri_commands!` at `commands/registry.rs:462` → handler installed `lib.rs:192`.
- **NO `set_ui_feature_flags` exists** (grep across `src-tauri` + `frontend` = 0). Read-only today.

### 2.3 The flag is a pure FRONTEND nav-visibility concern
- No backend behavior gates on `ui_feature_flags.automations_page`. The only backend reader is the accessor feeding the Tauri command (`harness_runtime_registry.rs:11,780-781`).
- The backend automations **engine** is a separate config — `AutomationsRuntimeConfig` (`runtime_config.rs:530`, `automations_config()` at `mod.rs:1813`), unrelated to the UI flag. Toggling the UI flag never enables/disables the backend scheduler; it only shows/hides the nav + view.

### 2.4 Frontend has TWO disconnected boot-time snapshots — both must be updated on a live toggle
- **(A) TanStack Query** `useFeatureFlags()` — `frontend/src/hooks/useFeatureFlags.ts:31-51`. `queryKey = FEATURE_FLAGS_QUERY_KEY = ["featureFlags"]` (`:14`), `staleTime: Infinity`, `retry: false`, one `invoke("get_ui_feature_flags")` at boot. **Drives nav-rail visibility + App render branch.** No invalidation exists anywhere.
- **(B) Zustand `uiStore.featureFlags`** — `frontend/src/stores/uiStore.ts:313` (state), seeded once at module load `:1060-1069`, setter `setFeatureFlags` `:951-962`. **Drives redirect-away guards** in `setCurrentView` (`:568-580`) and `switchToProject` (`:909-911`).
- Consumers: nav predicate `nav-items.ts:63` `visible: (flags) => flags.automationsPage` (flag only, ignores `taskCount`); App render gate `App.tsx:1299` + DEV `FeatureDisabledPlaceholder` `:1312`; App redirect effect `App.tsx:240-248` (**prod-only**); `isViewEnabled` `useFeatureFlags.ts:57-72`. No automations keyboard shortcut exists.
- **Live-toggle implication:** flipping only (A) changes nav + render but never redirects a user off a now-disabled view in DEV; flipping only (B) redirects but nav still shows. A correct live toggle updates **both** in one action.
- Client-side layering seam already exists: `applyFeatureFlagOverrides` (`useFeatureFlags.ts:27-29`) is an identity passthrough today — a ready-made hook for merging an override.

### 2.5 Settings UI + the persisted-setting pattern to copy
- Live Settings UI is **`SettingsDialog.tsx`** (rendered `App.tsx:1416`), tabbed. `SettingsView.tsx` is a legacy dispatcher — do not build there.
- Section registry: `settings-registry.ts` — `SETTINGS_SECTIONS` (`:50-71`), `SETTINGS_GROUPS` (`:38-46`: `harness, workspace, general, ideation, integrations, access, preferences`), id union (`:1-21`). Section→component map (all lazy): `SettingsSectionContent.tsx:118-165`.
- Toggle primitive: **`ToggleSettingRow`** (`SettingsView.shared.tsx:109-156`, shadcn `Switch` at `components/ui/switch.tsx`; note the `userIntentRef` guard `:124-135` against Radix stale `onCheckedChange`). Companions: `SectionCard`, `NumberSettingRow`, `SelectSettingRow`.
- **Persisted-settings template (copy this): Global Capacity.** `GlobalExecutionSection.tsx` (debounced 300 ms save `:56-74`) → `executionApi.updateGlobalSettings` (`api/execution.ts:213-221`) → `invoke("update_global_execution_settings", { input })` → `commands/execution_commands/settings.rs:297-376` → `global_execution_settings_repo.update_settings` → singleton table `global_execution_settings` (id=1 CHECK) → syncs in-memory `ExecutionState` (`settings.rs:322-326`) + emits `settings:global_execution:updated`. Repo: `sqlite_execution_settings_repo.rs:198+`. Migration: `v11_per_project_execution_settings.rs:43-67`.
- Sections are **self-contained** (load-on-mount + optimistic local state); there are **no** frontend listeners for `settings:*:updated` events (backend emits, nobody subscribes). A new section follows the same self-contained pattern.
- Restart-required fallback pattern (if a live toggle is rejected): `ExternalMcpSettingsPanel.tsx` `RestartNotice` badge (`:28-38`, `data-testid="restart-required-badge"`).

---

## 3. Architecture decision

### Decision — DB-persisted per-flag override, merged onto the cached baseline in `get_ui_feature_flags`, with a live dual-store frontend update. **RECOMMENDED.**

**Backend:** add a singleton overrides table (`ui_feature_flag_overrides`, id=1, one **nullable** bool column per user-exposable flag; NULL = "inherit baseline"). Make `get_ui_feature_flags` `app_state`-aware: read the OnceLock baseline (`default_ui_feature_flags()`), then layer the DB override per-field. Add `set_ui_feature_flag_override`. The OnceLock stays the immutable baseline; the DB row is the user delta.

**Frontend:** a new **Labs** section in the `preferences` group renders a `ToggleSettingRow` for Automations. On toggle: `invoke` the set command → on success, update **both** flag stores (TanStack cache + `uiStore.setFeatureFlags`) so nav, render, and redirect all react without restart.

**Why this over the alternatives:**
- It is the idiomatic RalphX settings pattern (mirrors `global_execution_settings` + repo + get/set commands + migration), so it is backend-readable if any future flag ever needs to gate backend behavior, and it is queryable/inspectable via SQLite like every other setting.
- It gives a true no-restart toggle because the flag is frontend-only (§2.3) — no in-process runtime cache needs syncing beyond returning merged flags (unlike execution settings, which must also sync `ExecutionState`).

**Rejected — localStorage-only override (lighter).** Persist the override in `localStorage` and merge it in `applyFeatureFlagOverrides` (`useFeatureFlags.ts:27`), zero backend changes; precedent exists (Accessibility/theme → `useThemeStore` localStorage, `AccessibilitySection.tsx:62-67`). Rejected as the primary because it diverges from the app's SQLite-settings convention, is invisible to backend/config tooling, and is per-WKWebView-profile. *If the team wants half the effort and accepts those tradeoffs, this is a viable pivot — it is frontend-only and the flag is frontend-only — but the rest of this spec assumes the DB path.*

**Rejected — in-memory `Arc<Mutex<..>>` override on AppState.** Not persistent across restart; only useful for session-scoped toggles. A Settings toggle must survive restart.

**Rejected — mutate/reset `LOADED_CONFIG_CELL`.** Impossible on stable Rust (write-once `OnceLock`, §2.1); would also fight config/env as source of truth.

**No pristine-seed bootstrap.** Unlike execution settings (`execution_settings_bootstrap.rs`), the overrides table is NOT seeded from config — config is the baseline, overrides are pure user deltas, and an empty table (all NULL) means "inherit config for every flag." Do not copy the seed bootstrap.

---

## 4. Data model

### **NEW:** table `ui_feature_flag_overrides` (singleton)
Migration via `python3 scripts/new_sqlite_migration.py ui_feature_flag_overrides` (registered in `infrastructure/sqlite/migrations/mod.rs`, bump `SCHEMA_VERSION`, validate with `scripts/validate_sqlite_migrations.py`). Template: `v11_per_project_execution_settings.rs` (singleton + CHECK + `INSERT OR IGNORE` default row).

| Column | Type | Purpose |
|---|---|---|
| `id` | INTEGER PK CHECK (`id = 1`) | singleton row |
| `automations_page` | INTEGER NULL | override: `NULL` = inherit baseline, `0`/`1` = explicit user value |
| `updated_at` | TEXT NOT NULL | RFC3339 |

- `INSERT OR IGNORE INTO ui_feature_flag_overrides (id, automations_page, updated_at) VALUES (1, NULL, <ts>)` seeds the empty singleton (all-inherit).
- Adding a future user-exposable flag = one `add_column_if_not_exists` ALTER migration (precedent: execution settings added columns via ALTER migrations). Keep columns nullable-inherit.
- **Do NOT** use a generic `key TEXT`/`value TEXT` table — the codebase favors typed columns, and an arbitrary `key` becoming a column/lookup is a fragile-string (rule 5) and CodeQL smell. Typed columns + a fixed enum (§5.1) keep untrusted strings out of the DB shape.

---

## 5. Backend changes

### 5.1 Overridable-flag enum (no fragile strings)
**NEW:** `enum UiOverridableFlag { AutomationsPage }` (domain), with a fixed mapping enum-variant → column. The `set` command accepts this enum, never a raw column string. This is the rule-5 / CodeQL guard: user/request input selects a variant, not a DB column name.

### 5.2 Repository (3-file pattern)
**NEW:** `UiFeatureFlagOverridesRepository` trait (`domain/repositories/`) + SQLite impl (`infrastructure/sqlite/sqlite_ui_feature_flag_overrides_repo.rs`) + memory impl (`infrastructure/memory/`). Register on both AppState instances (dual-AppState, constructed in `run_app_setup`, `application/app_setup.rs`). All access via `db.run(|conn| …)` (DbConnection rule). Methods:
- `get_overrides() -> AppResult<UiFeatureFlagOverrides>` (a struct of `Option<bool>` per flag; fresh read, NOT cached).
- `set_override(flag: UiOverridableFlag, value: Option<bool>) -> AppResult<()>` (upsert singleton; `value = None` clears → inherit).

### 5.3 Rewrite `get_ui_feature_flags` to merge baseline + override (fail-closed)
Change `commands/ui_commands.rs:24-37` from the pure sync fn to:

```rust
#[tauri::command]
pub async fn get_ui_feature_flags(
    app_state: tauri::State<'_, AppState>,
) -> Result<UiFeatureFlagsResponse, String> {
    let base = default_ui_feature_flags();               // OnceLock baseline (config/env)
    let overrides = app_state
        .ui_feature_flag_overrides_repo
        .get_overrides()
        .await
        .unwrap_or_else(|e| { tracing::warn!(?e, "flag override read failed; using baseline"); Default::default() });
    // Per-field: override.is_some() ? override : base. Only automations_page is overridable in v1.
    Ok(merge(base, overrides))
}
```

- **FAIL-CLOSED (stateful-workflow rule):** an override read error must fall back to the **config baseline**, NOT to hardcoded `Default`, and must NOT error the command (erroring would drop the frontend to `DEFAULT_FEATURE_FLAGS` and could hide/show the nav wrongly). Log + use baseline.
- Frontend `invoke("get_ui_feature_flags")` is unchanged (no args; Tauri injects `State`). The response is now `Result`, but `invoke` already returns a promise the queryFn awaits.
- **Ripple:** `get_ui_feature_flags` becomes `async` + `State`-taking. Update `ui_commands_tests.rs` (currently calls it directly, sync). Grep for any other direct callers before changing the signature.

### 5.4 **NEW:** `set_ui_feature_flag_override` command
```rust
#[tauri::command]
pub async fn set_ui_feature_flag_override(
    app_state: tauri::State<'_, AppState>,
    input: SetUiFeatureFlagOverrideInput,   // #[serde(rename_all="camelCase")] { flag: UiOverridableFlag, enabled: Option<bool> }
) -> Result<UiFeatureFlagsResponse, String>
```
Writes the override (`enabled = None` clears), then returns the freshly-merged flags (so the caller updates its stores from the authoritative merged result, not an optimistic guess). Register in `commands/mod.rs` + `register_tauri_commands!` (`commands/registry.rs`). Struct param → frontend invokes `{ input: { flag, enabled } }` (tauri-invoke-conventions: wrap under `input`, camelCase).

---

## 6. Frontend changes

### 6.1 API
**NEW:** `frontend/src/api/featureFlags.ts` (or extend an existing api module): `setUiFeatureFlagOverride({ flag, enabled }): Promise<FeatureFlags>` → `invoke("set_ui_feature_flag_override", { input: { flag, enabled } })` → `featureFlagsSchema.parse`.

### 6.2 **NEW:** `LabsSection.tsx` (preferences group)
- Register: add id `"labs"` to the union (`settings-registry.ts:1-21`) and `{ id: "labs", groupId: "preferences", label: "Labs" }` to `SETTINGS_SECTIONS` (`:50-71`); add lazy import + `section === "labs" && <LazyLabsSection/>` in `SettingsSectionContent.tsx:118-165`.
- Content: `SectionCard` with a short "Experimental features. Changes apply immediately." description + a `ToggleSettingRow` labeled "Automations" (`SettingsView.shared.tsx:109-156`). Read current value from `useFeatureFlags()` (source A).
- On toggle → `setUiFeatureFlagOverride({ flag: "automationsPage", enabled })`. On success, the **dual-store live update** (§6.3). Follow the debounced/optimistic pattern of `GlobalExecutionSection.tsx`; keep the `userIntentRef` Radix guard.

### 6.3 Dual-store live update (the load-bearing bit — §2.4)
On a successful `set`, apply the returned merged flags to BOTH sources in one handler:
```ts
const merged = await setUiFeatureFlagOverride({ flag: "automationsPage", enabled });
queryClient.setQueryData(FEATURE_FLAGS_QUERY_KEY, merged);   // (A) nav + render react; staleTime:Infinity needs explicit set
useUiStore.getState().setFeatureFlags(merged);               // (B) redirect-away guard (uiStore.ts:951-962)
```
- OFF→ON: nav item appears immediately (nav filter recomputes every render from source A — `LeftNavRail.tsx:122-124`).
- ON→OFF while the user is ON the automations view: `setFeatureFlags` redirects to `DEFAULT_APP_VIEW` (`agents`) in BOTH DEV and prod, avoiding stranding (the App.tsx redirect effect is prod-only, so relying on it alone would strand DEV users on a blank/placeholder view).
- Do NOT update only one store — nav and redirect would split.

---

## 7. Precedence & fail-closed semantics
- Effective value = **DB override if non-NULL, else config baseline** (baseline = struct default → YAML → env, resolved in the OnceLock).
- v1 keeps it simple: a non-NULL user override wins over env/YAML. RalphX is a single-user desktop app, so the operator and the user are the same person; a stale override is user-reversible (toggle again / clear). **Deferred (§12):** an "env-managed lock" where a present `RALPHX_UI_*` env var forces the value and disables the Settings toggle with a "managed by env" note — needs the backend to expose per-flag "env-forced" provenance, out of scope for v1.
- Override read failure → baseline (never hardcoded defaults, never a hidden nav, never a command error). See §5.3.

---

## 8. Edge cases
| Scenario | Handling |
|---|---|
| Fresh install, no override row | `INSERT OR IGNORE` seeds `automations_page = NULL` → merge returns baseline (`true` per current config) |
| Override read errors (DB hiccup) | Log, use baseline config, do not error the command (§5.3) |
| Toggle OFF while viewing Automations | `setFeatureFlags` redirects to `agents` (DEV + prod), user not stranded (§6.3) |
| Toggle ON | Nav item + view become available immediately; no restart |
| Unknown flag in `set` input | Rejected by `UiOverridableFlag` enum deserialization → typed error (rule 5) |
| Concurrent toggles | Singleton upsert, last-write-wins (acceptable for single user) |
| Config default later changes | Baseline shifts; a NULL override still inherits; a non-NULL override still wins until cleared |
| Additional flags | Not exposed in v1; each future flag needs its own nullable column, enum variant, and `ToggleSettingRow` |

---

## 9. Testing plan (TDD-first)
Rust via `scripts/test-rust-fast.sh pr` / `cargo nextest run --manifest-path src-tauri/Cargo.toml --lib --profile ci`; frontend via Vitest.

- **Migration test:** table created, singleton CHECK enforced, default row is all-NULL (template: `v11_*` test shape).
- **Repository tests (SQLite + memory parity):** empty → all `None`; `set(AutomationsPage, Some(false))` → `get` returns `Some(false)`; `set(.., None)` clears to `None`; upsert stays singleton.
- **Command tests:** `get_ui_feature_flags` merges baseline+override (override present → override wins; absent → baseline); fail-closed (repo error → baseline, command still `Ok`); `set_ui_feature_flag_override` persists + returns merged; camelCase serialization preserved (extend `ui_commands_tests.rs:21-22,54-55`); unknown-flag input rejected.
- **Frontend:** `LabsSection` renders the toggle reflecting current flag; toggling calls `set_ui_feature_flag_override` with `{ input: { flag, enabled } }`; on success BOTH stores update (assert `queryClient` cache for `FEATURE_FLAGS_QUERY_KEY` AND `uiStore.featureFlags`); OFF→ON shows `nav-automations`; ON→OFF from the automations view redirects to `agents`.
- **Extend existing coverage** (must keep passing): `LeftNavRail.test.tsx:173-179`, `useFeatureFlags.test.ts:84-91`, `uiStore.test.ts:1142-1312`, `App.test.tsx:495-517`, `App.navigation.test.tsx:21`.
- Zero clippy/lint/test warnings including pre-existing in touched files (rule 8).

---

## 10. Phased implementation
| Phase | Scope | Exit |
|---|---|---|
| **P1 — Backend** | Migration + `ui_feature_flag_overrides` table; `UiOverridableFlag` enum; repo (3-impl) + AppState wiring; rewrite `get_ui_feature_flags` (async + merge + fail-closed) with test updates; `set_ui_feature_flag_override` + registration | Migration/repo/command tests green; camelCase preserved; `get` merges override |
| **P2 — Frontend** | `api/featureFlags.ts`; `LabsSection.tsx` + registry/section-content wiring; dual-store live-update handler; tests | Interaction tests green (both stores update; OFF→ON nav; ON→OFF redirect); Tauri-native check |

Small enough to be one PR; split for review clarity. P2 depends on P1's command.

---

## 11. File-touch checklist
**Backend:** `infrastructure/sqlite/migrations/vYYYY…_ui_feature_flag_overrides.rs` (+ `_tests.rs`) · `migrations/mod.rs` (register + `SCHEMA_VERSION`) · `domain/repositories/mod.rs` (trait) · `infrastructure/sqlite/sqlite_ui_feature_flag_overrides_repo.rs` · `infrastructure/memory/…` · domain `UiOverridableFlag` enum · `application/app_setup.rs` (repo on AppState) · `commands/ui_commands.rs` (rewrite `get`, add `set`, `SetUiFeatureFlagOverrideInput`) · `commands/mod.rs` + `commands/registry.rs` (register `set`) · `commands/ui_commands_tests.rs` (async update).
**Frontend:** `api/featureFlags.ts` (**NEW**) · `components/settings/settings-registry.ts` (id + section) · `components/settings/SettingsSectionContent.tsx` (lazy + gate) · `components/settings/sections/LabsSection.tsx` (**NEW**) · tests listed in §9.

---

## 12. Constraints / proof obligations
1. Repo access via `db.run(|conn| …)` (DbConnection rule); no direct `conn.lock().await`.
2. Fail-closed: override read error → config baseline, never hardcoded defaults, never a hidden nav, never a command error (stateful-workflow rule).
3. No fragile strings: `set` takes the `UiOverridableFlag` enum, fixed variant→column mapping; user/request input never becomes a raw DB column or path (rule 5 + CodeQL).
4. Tauri invoke: camelCase, struct param wrapped under `input`; Zod schema matches serde casing.
5. `get_ui_feature_flags` signature change is a ripple — update all direct callers/tests; keep the frontend `invoke` call args unchanged (no args).
6. Live toggle updates BOTH frontend flag stores atomically (§2.4/§6.3) — a single-store update is a defect.
7. Tokio safety: async command + `db.run`; no `tokio::spawn` in sync context.
8. Stable Rust only; zero new clippy/test warnings incl. pre-existing in touched files (rule 8).
9. Design system: reuse `ToggleSettingRow`/`SectionCard`/`Switch`; no new WKWebView canvas tokens or inline-style `var()`; verify in `npm run tauri dev`, not just `dev:web`.

---

## 13. Open questions
1. **Section name:** "Labs" vs "Experimental" vs folding the toggle into the existing `general` group. Recommendation: a dedicated **Labs** section in `preferences` (beside Accessibility), so future flags have a home. (Low-stakes; implementer's call.)
2. **Env-managed lock** (§7) — expose per-flag "forced by env" provenance and disable the toggle when set? Deferred; decide if operator-lock semantics are ever needed on this single-user app.
3. **localStorage pivot** (§3 rejected alt) — if backend churn is unwanted and the frontend-only tradeoffs are acceptable, this collapses to ~half the work (edit `applyFeatureFlagOverrides` + `LabsSection` + dual-store). Flagged for an explicit go/no-go before P1.
