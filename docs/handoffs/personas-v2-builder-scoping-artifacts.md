# Personas v2 — Scoped Personas, Builder-in-Agents-View, Folder Context, Versioned Persona Artifacts

> Status: **IMPLEMENTED — feature-complete on PR #779; final validation and review are in progress. The converged specification passed three adversarial rounds (R1: 2× Fable + gpt-5.6-sol; R2: 2× Fable; R3 verification: 1× Fable + gpt-5.6-sol). Owner answered all open questions 2026-07-16 (§11), including the reference-not-copy revision for folder context.**
> Date: 2026-07-16
> Scope basis: research + two adversarial rounds against `main` @ 88880f0c1; every file:line anchor code-verified by at least one reviewer.

---

## 1. Executive Summary

Personas v1 shipped a working but siloed builder: a custom ingestion screen inside Settings, a copy-at-pick ingest store, a one-per-project builder conversation, in-place persona mutation with no history, global-only personas, and read-only drafts. This spec replaces that with:

1. **Scope-aware building** — "Build with Agent" asks Global vs. Project. Project builds run inside that project's context; Global builds run in a **projectless (standalone) conversation** with a **private per-conversation workspace** as CWD. The extractor sees only attached context — enforced at the MCP filesystem-tool layer (read roots = workspace + attached folders, read live via `fs_*`), not by copying. (Owner revised R1's original "copy to workspace" wording 2026-07-16: reference-not-copy; only composer attachments are synced into the workspace.)
2. **Builder lives in the Agents view** — the Settings custom ingestion screen is dropped (final cutover phase, after its replacement exists). "Build with Agent" deep-links into the Agents new-conversation screen with mode **Persona** preselected. Context arrives through the normal composer (Add Files / **Add Folder**), and the extractor **analyzes context and interviews the user** through the existing `ask_user_question` banner UX.
3. **Generic composer folder picker** — "Add Folder" under "Add Files"; any machine folder; persists as a conversation-level reference; injected as a system-prompt hint on every spawn/resume.
4. **Project-scoped personas** — `personas.project_id` (NULL = global); Settings filters by scope; pickers offer global + current-project.
5. **Manual editing of any persona** — drafts become editable.
6. **Versioned persona artifacts** — every persona content write appends to an immutable artifact version chain, surfaced as a **Persona tab** in the Agents artifact pane. **1 builder conversation ↔ 1 persona** enforced end-to-end, including across approval.

---

## 2. Current State (condensed, file-anchored)

### 2.1 Personas today

| Fact | Anchor |
|---|---|
| `personas` table: `id, slug, name, description, content, status(draft\|active\|archived), version, content_hash, source_session_id, source_persona_id, source_content_hash, source_json` — **no `project_id`** | `v20260711151804_personas.rs:10-27`, `v20260715172058_persona_update_draft_provenance.rs` |
| Versioning is in-place mutation (`version = version + 1`); no history | `persona_update_approval.rs:123-137` |
| Active-slug uniqueness: `idx_personas_slug_live` (`UNIQUE(slug) WHERE status='active'`); **unique-violation mapping string-matches the old index name** (`map_live_slug_unique_error`) | `v20260715172058:23-27`, `sqlite_persona_repo.rs:83-93` |
| `save_persona_draft` HTTP handler: caller-session header resolves the bound draft, **but missing header OR header→missing/non-builder conversation falls through (via `.filter()`) to an unrestricted update by client `draft_id`** (fail-open). Sole production client is internal MCP `persona-tools.ts`, which **already always sends the header** (throws client-side without `conversationId`) | `handlers/personas.rs:31-119` (:39-51,:90-111), `persona-tools.ts:99-109`, `http_server/mod.rs:173` |
| **Binding across approval is non-uniform**: `apply_seeded_draft` deletes the draft + clears builder bindings (:144-150); `approve_persona_as_new` clears bindings (:243); **plain `approve_persona` leaves `builder_draft_id` set** (`personas/mod.rs:147-167`). Post-approval `save_persona_draft` on the cleared paths silently creates a new bound draft (`handlers/personas.rs:69-89`) | verified both rounds |
| Content writers: exactly six — `save_persona_draft` path (`update_draft`/`create_bound_draft`), `update_persona`, `create_persona_draft`, `approve_persona` (internal `update_content` re-write before `set_status`), `apply_seeded_draft`, `approve_persona_as_new` (recomposes content on slug change, raw SQL in `run_transaction`). `reseed_persona_draft` writes only `source_content_hash` | `personas/mod.rs:75-182`, `persona_update_approval.rs:97-243` |
| Approval-module slug checks are raw SQL, scope-unaware (`active_slug_owner`, `ensure_new_slug_available`) | `persona_update_approval.rs:64-94` |
| `archive_persona` clears **only** `persona_id` bindings (`clear_persona_bindings_sync`) | `personas/mod.rs:196-202`, `sqlite_chat_conversation_repo.rs:143-151` |
| Builder = mode `PersonaBuilder` → `ralphx-persona-extractor` (MCP-only fs tools; native tools `[TaskList]`; **no delegation block**; Codex lane `shell_tool: false`, Codex MCP locked to `danger-full-access`) | `chat_service/mod.rs:719-730`, `agents/ralphx-persona-extractor/agent.yaml`, `config/harnesses/claude.yaml:26` |
| **`persona_builder` is rejected at six sites**: start-service parse (`helpers.rs:66` → `reject_persona_builder_workspace_mode`, `agent_conversation_workspace.rs:1009-1015`), `unified_chat_commands.rs:2739` (parse copy), `:2963` (mode-switch — blocked both directions with the guard at :2957-2961), `automation/service.rs:383,2266`, `automation/provisioning.rs:118` | verified |
| **Ingest liveness gate**: `ensure_persona_builder_has_live_context` (`chat_service/mod.rs:1516-1547`; call sites :4434,:4892,:6310) — **passes on a live bound draft alone** (refine builds legitimately run with zero ingest content); queue path blocks separately via `builder_context_error` (`chat_service_queue.rs:530-575,1018-1040`) | verified |
| Old/new builder conversations are **shape-identical in the DB** (no column distinguishes ingest-era builds); the only observable discriminator is a live on-disk ingest store | verified |
| Ingest store: copy-at-pick, caps 256KiB/8MiB/500/depth-12; **binary-extension + dotfile denial** (`persona_ingest.rs:266-269,504-539`); no cleanup path exists | `persona_ingest.rs`, `persona_ingest_batch.rs` |
| Persona injection: `resolve_persona_for_send` (6 call sites) → `render_persona_block` → `apply_persona_overlay`; Project-gated; **`Explicit` branch returns before mode suppression**; repo errors are typed (fail closed) | `persona_resolver.rs:51-103` |
| Manual editing: Active editable; drafts read-only; `PersonaService::update_draft` is the agent write path | `PersonasManagementSection.tsx:194-241`, `personas/mod.rs:127-140` |
| Seeding internals for refine (`persona_builder_commands.rs:112-186`) touch only db + persona/conversation repos — **clean move into `PersonaService`** (already constructed with those three at `agent_conversation_start_service.rs:342-349`). `SavePersonaDraftInput` has no scope field | verified |
| Per-run attribution on `agent_runs` + `PersonaChip` (hidden for non-project contexts AND `persona_builder` mode: `IntegratedChatPanel.tsx:1543-1547`); `PersonaPickerControl` self-fetches via `usePersonas()` and is used **only** by the start composer | `PersonaPickerControl.tsx:31`, `AgentsStartComposer.tsx:1256-1269` |

### 2.2 Conversations & workspaces today

| Fact | Anchor |
|---|---|
| Project association = `(context_type, context_id)`; `Project` ⇒ `context_id` is the project id; conversations cannot exist without a project (`StartAgentConversationInput.project_id: String`, hard-fail :247) | `chat_conversation.rs:76-95,374-405`, `agent_conversation_start_service.rs:68,247` |
| `start()` **reuses** a seeded conversation via `input.conversation_id` (`:389-409`, ownership validated as Project-owned at :398-405; parent check :426-433); creates only when none supplied | verified |
| `CreateAgentConversationInput.context_id: String` required; exhaustive per-context constructor match | `unified_chat_commands.rs:10240-10301` |
| CWD resolution: exhaustive match on `ChatContextType`, every arm needs a project; failure silently falls back to `default_working_directory` | `chat_service_context.rs:1712-1912` |
| `resolve_mcp_filesystem_read_roots` (`chat_service_context.rs:1602-1699`): branches on `Option<project_id>` + mode, NOT `ChatContextType`. **PersonaBuilder arm keys on mode alone and early-returns** (ingest root or `[]`); **`project_path == CWD` dedup returns `[]`** (:1694-1696) — written assuming CWD is implicitly readable |
| Read-root wiring reaches spawn at 2 sites (`chat_service/mod.rs:3707`, `chat_service_queue.rs:1569`); `build_mcp_runtime_context` has **7 construction sites** (`chat_service_context.rs:2466,2560,2739,3311,3558,3671,4442`) and does **not** receive `agent_mode` (:2163-2196); `McpRuntimeContext` derives `Default`. **Resume-path runtime contexts carry `conversation_id = None`** (`chat_service_context.rs:244,532,3558`), and the Codex lane substitutes `context_id` (the project id) for it — the caller-session header is NOT reliably the conversation id on resume today | verified |
| **Queued-message context resolver is Project-only**: `resolve_queued_project_agent_context` (`chat_service_queue.rs:503-516`, consumed at :1562) — any non-Project conversation's queued messages resume with default context (mode/agent/workspace lost) | verified |
| MCP config: per-spawn temp file + `--strict-mcp-config`; MCP servers are stdio children per CLI run (no cross-conversation sharing); Claude Task subagents share the parent's MCP server env; env-fallback exists in `hydrateRalphxRuntimeEnvFromCli` (`runtime-context.ts:89-92`) — flags must ride CLI args, never process env | `claude/mod.rs:723-744`, `runtime-context.ts:20-43,89-92` |
| Conversations are archived, never deleted; archive flow **hardcodes `ChatContextType::Project`** when stopping the agent | `agent_conversation_archive.rs:30,189` |
| Restart recovery enumerates **Project only** (`chat_resumption.rs:252,274,336,438`); provider-pause requeue (`chat_service_handlers.rs:64-70`) and agent-waiting eligibility (`chat_service_streaming.rs:78-86`) are `matches!` excluding anything new; `message_queue.rs:581-590` string-dispatch `_ => return 0` (already omits `branch_update`); `chat_service/mod.rs:2677-2709` wildcard | verified |
| Sidebar: per-project backend query (`agent_sidebar_commands.rs:258-266`); `list_recent_resumable_by_context_type` **exists** on the repo trait (`chat_conversation_repository.rs:98`) — partial reuse candidate for standalone enumeration | verified |
| Frontend: main region gates on `activeProjectId && …` (`AgentsConversationMainRegion.tsx:132`); selection invalidated on contextId ≠ activeProjectId (`useAgentsSelectionModel.ts:48-66`); `IntegratedChatPanel.projectId: string` required (:189,369,495); **`useStartAgentConversation.ts` hardcodes `"project"` at :312,:377,:388,:412 and `projectId` at :459; seeded-conversation trigger `requiresSeededConversation` at :348**; registry has no `standalone` (`chat-context-registry.ts`) | verified |
| Composer: `project.value: string` drives assist/skills/plan-refs (`AgentComposerSurface.tsx:159,232,398-435`); pickers `disabled` when zero projects; Team toggle renders unconditionally, `validate_native_team_intent` is harness-only (`agent_conversation_start_service.rs:184-188`); Team-mode change restricted to Project context (`unified_chat_commands.rs:10336-10338`) | verified |
| Start composer: reset effect (`:398-400`, deps `[defaultProjectId, projects]`) refires on query identity churn; draft consumed **once** (`:402-424`) — a "locked" flag must be lifted into composer-local state at consumption or the guard has nothing to read later; `AgentStartConversationDraft.projectId: string` | `AgentsStartComposer.tsx`, `agentSessionStore.ts:101-109` |
| Deep link precedent also calls `setFocusedProject` + `clearSelection`; Settings is a modal (`openModal`/`closeModal`) | `useAgentConversationQuickAction.ts:34-57`, `uiStore.ts:346-348,553-574` |
| Mode lists differ (start vs conversation); conversation chip renders `activeOption?.label ?? "—"`; locked-mode menu leaves "Ideation" enabled; `AGENT_START_MODE_OPTIONS` has no `requiresProject` metadata | `agentStartModeOptions.ts:3-14`, `agentConversationMode.ts:8-43`, `AgentComposerSurface.tsx:2403-2405`, `AgentsActiveConversationPanel.tsx:1620-1628` |
| Old builder conversations already render in the sidebar with a "Persona Builder" badge | `AgentsSidebar.tsx:126-133,2618` |
| `chat_messages.project_id/session_id/conversation_id` all nullable; message-persistence context derivation is an exhaustive match | `v1_initial_schema.rs:512-525`, `chat_service_streaming.rs:111-156` |
| Chat mode agent `ralphx-general-explorer` has **native** Read/Grep/Glob (`readonly_tools`) + `delegate_start`; `delegate_start`'s non-ideation caller branch trusts a model-supplied `parent_session_id` (`coordination/mod.rs:306-320`) | verified |

### 2.3 Composer attachments today

| Fact | Anchor |
|---|---|
| "+" menu: single "Add files" row; 5 files / 10MB; composer-side extension/type allowlist | `AgentComposerSurface.tsx:1936-2018`, `chatAttachmentFiles.ts:1-43` |
| Upload → hash-addressed app-owned storage → DB rows; `upload_chat_attachment` is conversation-id-keyed, context-agnostic | `chat_attachment_storage.rs:14-27`, `chat_attachment_commands.rs:77-119` |
| Injection one-shot per turn; text inlined; **binaries injected as app-storage path + "Use the Read tool"** — dead end for the extractor (no native Read), and the storage dir is outside any enforced root | `chat_service_context.rs:2029-2074` |
| Standing-context precedent: persona block re-injected every build/resume; Codex composition fails soft (skip logged) | `chat_service_context.rs:225,249`, `codex/mod.rs:444-476` |
| Pre-send attachments live as draft-local `File` objects | `chatStore.ts:52-63` |

### 2.4 AskUserQuestion interview seam

RalphX-owned MCP tool `ask_user_question` (single or ordered `questions[]`) → HTTP long-poll → `agent:ask_user_question` → `QuestionInputBanner` above the composer → `resolve_user_question` unblocks. Pending questions survive restarts, suppress the idle-stdout kill timer, key on conversation/session id only, un-mode-gated in every `IntegratedChatPanel` host; extractor already has the grant. Anchors: `question-handler.ts:159-346`, `handlers/questions.rs:14-196`, `question_state.rs:252-447`, `QuestionInputBanner.tsx`, `IntegratedChatPanel.tsx:1981-1995`, `agents/ralphx-persona-extractor/agent.yaml:10`.

### 2.5 Artifact version-chain infrastructure

Immutable forward chains; owner holds the tip pointer. Verified caveats: `materialize_spec_artifact` is sequential async (NOT transactional — ours must be); sync helpers (`create_sync`/`create_with_previous_version_sync`, `sqlite_artifact_repo.rs:127-233`) join `db.run_transaction` cleanly (precedent: both approval fns already call `clear_builder_draft_bindings_sync` on the same conn). **Chains are strictly linear** — `resolve_latest_sync` walks single-successor links (:127-140); there is no multi-parent graft. **`metadata_json` is serialized exclusively from `metadata.team_metadata` in both sync helpers** (:167-170,209-212). `get_artifact_version_history` exists but returns no `created_by` (`sqlite_artifact_repo.rs:678`, `artifact_commands.rs:148`). `ArtifactType` closed enum + closed TS schema (`types/artifact.ts:27`); `prd-library` bucket policy excludes personas (`v25_seed_artifact_buckets.rs:21`). Migrations are plain Rust `execute_batch` fns; rusqlite 0.32 bundled (SQLite ≥3.45) supports expression + partial unique indexes. Mode-gated tab precedent: AUTOMATION_TAB + exhaustive `ARTIFACT_TAB_UNAVAILABLE_REASONS` (`AgentsArtifactPane.tsx:359-363,405,1073`).

### 2.6 Pre-existing security/infra gaps this spec must handle

| Gap | Anchor |
|---|---|
| MCP `fs_*` tools never consult read roots; `getAllowedFilesystemRoots()` unconditionally includes `process.cwd()`; containment is lexical (no realpath) | `filesystem-tools.ts:284-299`, `path-policy.ts:17-19,68-90` |
| Permission bridge: read-tool auto-approval trusts `{PWD, cwd, ~/.reefagent/agents}` + memory files; **Bash branch delegates to the same `isTrustedReadPath`**; lexical checks | `permission-handler.ts:93-125,173,205-236` |
| `PRAGMA foreign_keys` never enabled in production — FK actions are inert; `delete_project` is a bare row delete | `connection.rs:31-53`, `project_commands.rs:451-458` |
| 15 agents carry `fs_*` grants; worktree/ideation conversations get non-empty informational roots today | `agents/*/agent.yaml`, `chat_service_context.rs:1694-1698` |
| The caller-session header is unauthenticated loopback HTTP — anything reaching :3847 can spoof it (pre-existing posture; stated as a trust boundary, not fixed here) | `handlers/personas.rs` |

---

## 3. Requirements → Design Mapping

| # | Requirement (owner's words, condensed) | Design |
|---|---|---|
| R1 | Settings "Build with Agent" asks Global vs. Project; Global ⇒ private workspace, agent limited to attached context (revised: enforced live references, not copies); new-conversation screen gains projectless toggle (landable standalone) | D3, D4, D9, UX-2/3 |
| R2 | Builder analyzes context + project and interviews via AskUserQuestion UX; drop the ingestion screen; Settings redirects to Agents new-conversation with mode Persona | D4, D5, UX-3/4 |
| R3 | Composer folder picker "Add Folder"; persists as conversation reference; system-prompt hint | D6, UX-5 |
| R4 | Personas scoped per-project or global | D2, UX-1 |
| R5 | Any persona manually editable | D7, UX-1 |
| R6 | Builder conversation produces a versioned Persona artifact tied to the settings persona; 1 conversation ↔ 1 persona | D8, UX-6 |

---

## 4. Design Decisions

### D1 — Feature flags & rollout posture

- `agent_personas` (existing) gates persona behavior; folder references are always available; `standalone_conversations` gates D3.
- The old Settings builder path stays functional until the Phase 7 cutover. **One acknowledged exception:** Phase 0 enforcement changes legacy *refine* builds immediately — they currently read the whole project via the implicit-CWD hole; post-Phase-0 they are deny-all on fs tools (bound-draft-only builds have no ingest store). This is the intended hardening of a security hole, called out in Phase 0's exit criterion and test fixtures. The old extractor prompt leads with `fs_*` calls and would open every roots-empty refine build on denials — the prompt touch-up is therefore a mandatory Phase 0 task (0.5), not contingent; interview/draft tools are unaffected.

### D2 — Persona project scoping

- Migration `personas.project_id TEXT NULL` (+ index). NULL = global; existing rows stay NULL. FK actions are inert in production (2.6) — **deletion integrity is app-level**.
- Scoped active-slug uniqueness: `CREATE UNIQUE INDEX personas_active_slug_scoped ON personas(slug, IFNULL(project_id,'')) WHERE status='active'` (feasible: 2.5). **Update `map_live_slug_unique_error`** (`sqlite_persona_repo.rs:83-93`) — it string-matches the old index name and would regress friendly conflict errors to raw DB errors.
- **All slug checks scope-aware**: `ensure_live_slug_available`/`ensure_active_slug_available` AND the raw-SQL `active_slug_owner`/`ensure_new_slug_available` (else project-scoped approve-as-new falsely conflicts with a same-slug global persona).
- **Bindability** (single shared predicate): `persona.project_id IS NULL OR == conversation_project_id`. Enforced fail-closed at bind time, start time, and send time in **every persona-returning branch including `Explicit`** (which today returns before mode suppression). Mismatch ⇒ suppress with `persona_skipped_reason = "project_scope_mismatch"`. Repo errors stay typed (abort the send; never a silent persona-less send).
- **Project deletion (app-level)**: `delete_project` gains a sweep — drafts hard-deleted, actives archived, and all three binding columns (`persona_id`, `builder_draft_id`, `builder_result_persona_id`) cleared for affected conversations.
- Repository API: `list_personas` takes an explicit scope filter enum — `All` (Settings list), `GlobalOnly`, `GlobalAndProject(id)` (pickers) — a single `Option<ProjectId>` cannot express all three shapes. `PersonaPickerControl` self-fetches via `usePersonas()`; the scope param threads through that hook (project context exists at both hosts).

### D3 — Standalone (projectless) conversations

**New `ChatContextType::Standalone`, self-keyed (`context_id = conversation_id`).** The cheaper alternative (hidden scratch project) is catalogued in Open Q3; recommendation stands: honest Standalone, full cost enumerated below (the compiler sweep is insufficient — several consumers key on strings, `Option<project_id>`, or hardcode `Project`).

**Backend checklist (Phase 4a):**
1. Enum variant + serialization + compiler sweep; invalid arms ⇒ typed errors.
2. **Named non-compiler sites** (each an explicit task item): `message_queue.rs:581-590` (convert the string map to shared exhaustive dispatch — it already silently omits `branch_update`), `chat_service/mod.rs:2677-2709`, `agent_conversation_archive.rs:189` (use the conversation's real context when stopping the agent), **`chat_resumption.rs:252,274,336,438`** (restart recovery must enumerate Standalone or interrupted standalone conversations are never recovered), **`chat_service_queue.rs:503-516,1562`** (`resolve_queued_project_agent_context` is Project-only — a standalone conversation's queued/requeued messages would otherwise resume with default context, losing mode, agent identity, bound draft, and workspace), **`chat_service_handlers.rs:64-70`** (provider-pause requeue includes Standalone — else messages are dropped instead of requeued on usage-limit pauses), **`chat_service_streaming.rs:78-86`** (agent-waiting eligibility includes Standalone — else the interview never records `agent_waiting` and the notification system is blind to it), plus a fresh `rg 'ChatContextType::Project'` / `rg '"project"'` audit over `src-tauri/src` and `plugins/`.
3. `resolve_working_directory`: `Standalone` ⇒ private workspace root; missing/invalid ⇒ typed spawn error (never `default_working_directory`).
4. `resolve_mcp_filesystem_read_roots`: explicit standalone arm ⇒ `[private_workspace_root]`. Precedence with the PersonaBuilder mode arm is defined in D9.
5. `create_agent_conversation`: standalone arm — `context_id` becomes **optional** in `CreateAgentConversationInput` (ignored for standalone); backend generates the id and self-keys; new `ChatConversation::new_standalone()` constructor (context_id derived from the generated row id).
6. Seeded-conversation ownership rule in `start()`: a seeded standalone conversation is valid iff `context_type == Standalone && context_id == conversation.id && input.project_id == None` (mirror of the Project checks at `agent_conversation_start_service.rs:398-405`).
7. Sidebar enumeration: evaluate reusing `list_recent_resumable_by_context_type` (trait :98); add a dedicated method only if archived-filter/ordering needs differ; second enumeration pass in `agent_sidebar_commands.rs`.
8. `StartAgentConversationInput.project_id: Option<String>`; `None` requires flag + mode allowlist (`chat`, later `persona_builder`); **Team intent rejected** (typed); **`delegate_start` rejected for standalone callers** (typed) — the non-ideation caller branch trusts a model-supplied parent session and would otherwise be an unenforced escape hatch out of the workspace.
9. Persona binding for standalone: out of scope v1 (resolver stays Project-gated; picker hidden in UI).

**Honest enforcement statement for standalone Chat:** Chat mode maps to `ralphx-general-explorer`, and D9's MCP-layer enforcement bounds `fs_*`. Claude retains native Read/Grep/Glob but removes their preapproval, so out-of-root access crosses the permission-prompt boundary. Codex uses backend-owned `on-request`/`workspace-write`; because RalphX launches non-interactive `codex exec`, an action that requires fresh approval fails closed and surfaces the error instead of opening a prompt. The *hard* "sees only attached context" guarantee applies to the extractor, whose native filesystem surfaces are disabled — i.e., to persona builds. US-5's flow reflects these provider-specific boundaries.

**Private workspace service (Phase 4a):** `standalone_workspace.rs`, generalized from `persona_ingest.rs` (hash-addressed root `app_data_dir/standalone_workspaces/conversation-<sha256[:12]>/`, symlink rejection, `require_under_root`, manifest, caps → config). **This service also serves project-context persona builds (D4)** — creation is keyed on "standalone context OR persona_builder mode," idempotent, triggered by `create_agent_conversation`/`start()`/attach-time materializer, whichever runs first. Lifecycle:
- Archive does NOT delete the workspace; restore never hits the fail-closed error.
- Reclamation v1 = **crash-orphan cleanup only** (startup sweep deletes workspaces whose conversation id no longer exists). Age-based deletion is **deferred** (it would brick restore of old archived conversations — conversations are never deleted — and persona-ingest has run without cleanup since v1 without harm). Resolves Open Q4.

**Frontend checklist (Phase 4b):**
1. `chat-context-registry.ts` standalone entry (full agent-type-map checklist).
2. **`useStartAgentConversation.ts`** — owns the entire seed/upload/start flow and hardcodes `"project"` at :312,:377,:388,:412 and `projectId` at :459: optimistic conversation, seeded creation, store keys, and `startInput` must all be standalone-aware (upload itself is context-agnostic).
3. Main region + selection model accept project-less selected conversations (`AgentsConversationMainRegion.tsx:132`, `useAgentsSelectionModel.ts:48-66`).
4. `IntegratedChatPanel.projectId: string | null` + internal hook short-circuits; `AgentComposerSurface.project.value: string | null` + assist disable + "No project" line rendering (no sentinel ids reaching backend queries).
5. Start composer: "No project" item inside the project picker (enabled at zero projects); mode gating (new `requiresProject` metadata on mode options); captions; Team toggle + persona picker hidden; reset-effect guard (D5).
6. Sidebar "No project" group + query keys/invalidation/running-state.
7. `AgentStartConversationDraft.projectId: string | null`.
8. Tests include: standalone auto-titling (session namer with `project_id = None`), restart recovery, provider-pause requeue, queue-count visibility.

### D4 — Builder scope semantics (Global vs. Project builds)

Both build kinds are ordinary `persona_builder`-mode conversations through the standard start pipeline:

| | Project build | Global build |
|---|---|---|
| Conversation | `ChatContextType::Project`, mode `persona_builder` | `ChatContextType::Standalone`, mode `persona_builder` |
| CWD | project `working_directory` | private workspace root |
| Enforced read roots (D9) | `[project working_directory, private workspace] + live folder refs` | `[private workspace root] + live folder refs` |
| Attached files | copied into the workspace (bounded by composer caps; content NOT inlined in builder mode — path-referenced, read via `fs_*`) | same |
| Attached folders | **live references** (D6) — never copied; agent reads on demand from the real location | same (isolation comes from enforced roots, not copying) |
| Persona scope | that project — fixed at creation | Global — fixed |
| Harness | Claude or Codex | Claude or Codex; the Codex extractor retains its MCP-compatible launch policy while filesystem access remains bounded by enforced MCP roots |
| Team intent | **rejected** (typed) for Persona mode in any context — team overlays on a builder conversation are undefined territory | rejected (standalone rule) |

**Mode-rejection lift/keep table (the six sites, 2.1):**

| Site | Action |
|---|---|
| `agent_conversation_start_service` parse (`helpers.rs:66`) | **Lift** — allow `persona_builder` through the start pipeline (flag-gated) |
| `unified_chat_commands.rs:2739` (create_agent_conversation parse) | **Lift** — needed for seeded builder conversations |
| `unified_chat_commands.rs:2963` (mode-switch) | **Keep** — builder conversations stay mode-locked both directions |
| `automation/service.rs:383,2266`, `automation/provisioning.rs:118` | **Keep** — automation never provisions builder conversations |

**Private workspace for every builder:** the start pipeline (and attach-time materializer) idempotently creates the per-conversation workspace for **any** `persona_builder` conversation regardless of context type, under the same `standalone_workspaces/conversation-<hash>` root (D3's service; crash-orphan sweep generalizes for free).

**Context-access rules (reference-not-copy; owner decision 2026-07-16):** folders are **never copied** — a folder reference is a row + an enforced read root + a path hint, identical for project and global builds. Isolation for global builds comes from D9 enforcement (roots = exactly `[workspace] + attached folders`), not from snapshotting. Consequences: no folder-tree caps in the new flow (the v1 ingest caps survive only in the legacy store until Phase 7); folder content is read live (a folder deleted/renamed mid-build surfaces as `fs_*` read errors — acceptable); symlink-escape protection is D9's realpath containment plus registration-time symlink-root rejection.
- **Composer attachments — text only for persona builds, copied into the workspace.** Attachments live in app-owned attachment storage, which is outside the enforced roots, so they're the one thing materialized (idempotent sync at `start()` for pre-send uploads, at attach time after; bounded by composer caps — 5 files × 10MB — no new caps). `fs_read_file` is text-only by design (NUL rejection, UTF-8 decode, 256KiB read cap — `filesystem-tools.ts:12,36,515`) and the extractor has no native Read, so **binary attachments are rejected at attach time** in builder conversations ("The persona builder can only read text context — PDFs/images aren't supported"). Vetting honesty: the composer's `accept` attribute is advisory — real enforcement is size+count (`chatAttachmentFiles.ts:29-43`) + safe-name normalization + the materializer's own UTF-8/no-NUL check.
- **No content inlining in builder mode:** persona-builder conversations suppress the per-turn attachment content injection; the injector emits **workspace path references** instead ("read with `fs_read_file`"), keeping prompts lean and letting each turn choose what to read (all 5 production call sites of `format_attachments_for_agent` have mode + `app_data_dir` reachable — small signature change). Generic (non-builder) conversations keep today's inline behavior — changing that is out of scope.

**Seeded-refine provenance:** `StartAgentConversationInput.source_persona_id: Option<String>` (valid only with mode `persona_builder`); the start service validates the source and creates the seeded bound draft at conversation creation (seeding internals move from `create_persona_builder_conversation_for_state` into `PersonaService` — verified clean, 2.1). **Refine locks scope = source persona's scope**: the deep link skips the scope chooser, and the start service rejects `source_persona_id` whose scope ≠ the conversation's scope (else approve-as-new would silently rescope the lineage). `SavePersonaDraftInput`/`create_bound_draft` gain the scope field for stamping.

**Ingest-gate retirement (unconditional):** `ensure_persona_builder_has_live_context` (all three send-path call sites) and the queue-path `builder_context_error` mechanism are removed outright in Phase 5 — old- and new-style builder conversations are DB-identical, so a conditional retirement is unimplementable. Legacy UX loses nothing (the old `PersonaBuilderView` still gates chat behind ingest liveness in the frontend until Phase 7). Legacy/new divergence lives only at read-root resolution (D9's fall-through).

### D5 — Interview-driven extraction + Settings redirect

- Extractor prompts rewritten: **Analyze** (attachments + folder refs + roots; project builds sample the repo) → **Interview** (`ask_user_question` batches; ≤3 rounds before first draft, prompt-enforced; backend cap = Open Q1) → **Draft & iterate** (`save_persona_draft` early).
- No new frontend work for the interview (2.4).
- **Settings "Build with Agent"** → scope chooser (UX-1b) → deep link done correctly: `setStartConversationDraft({ projectId: <id | null>, projectLocked: true, mode: "persona_builder", sourcePersonaId? })` → `closeModal()` → `setFocusedProject(projectId ?? null)` → `clearSelection()` → `setCurrentView("agents")`.
- **Project lock:** `projectLocked` on the draft; at consumption the lock is **lifted into composer-local state** (the draft is consumed-once — a guard reading the store draft later would find nothing) and persists until unmount/explicit unlock; while locked, the picker is disabled and the reset effect (`AgentsStartComposer.tsx:398-400`) is gated.
- Mode "Persona" in **both** lists (start, flag-gated; conversation, label-only since switching is backend-blocked). Locked-mode menu fix: for locked modes (`automation`, `persona_builder`) **every** option is disabled (today "Ideation" leaks enabled).
- "Refine with Agent" row action: same deep link + `sourcePersonaId`, scope chooser skipped (D4).
- Old `PersonaBuilderView` path remains until Phase 7.

### D6 — Composer "Add Folder" (generic feature)

- **Data model:** `conversation_folder_references (id, conversation_id, folder_path, display_name, created_at, removed_at NULL)`. Soft cap 5 live refs (config). Folders are pure references — never copied (D4), so no materialization columns.
- **Registration:** realpath-canonicalize → `validate_absolute_non_root_path` → reject symlink roots → reject roots under `app_data_dir` → store canonical; reject newlines/control chars in `display_name`; re-validate at every sink.
- **Pre-conversation picks:** draft-local state (mirroring pre-send `File`s); registered against the seeded conversation when one exists — `requiresSeededConversation` extends to `… || folders.length > 0`.
- **Injection:** `<referenced_folders>` block re-rendered every build/resume via ordered overlays (`apply_prompt_overlays(system_prompt, [persona_block, folder_refs_block])`); XML-entity escaping of `<`, `>`, `&` in paths/names. Codex parity rides the per-turn instruction block; the fail-soft early-return must log a `folder_refs_skipped` reason (silent drop is not acceptable; full attribution parity is a non-goal).
- **Access:**
  1. Folder refs append to read roots for **Project** conversations (Phase 3) and for **persona_builder conversations of any context** (Phase 5 rewires the builder arm). Standalone *chat* keeps no folder refs in v1 (non-goal). Threaded **after** the existing arms of `resolve_mcp_filesystem_read_roots` (survives its early-returns).
  2. Permission bridge: new **read-tools-only** predicate (`isTrustedReadRootPath`, realpath'd), consulted only by the Read/LS/Grep/Glob branch — never the Bash branch (which shares `isTrustedReadPath` today).
  3. Enforcement is D9-flag-gated; adding a ref never flips a normal conversation into enforced mode.
- **Builder-mode gating (cross-phase seam):** until Phase 5 rewires the builder read-root arm, the "+ Add folder" row is **hidden for `persona_builder` conversations** — otherwise (Phase 3 + Phase 0 world) the hint would be injected while the root is unreachable, and every read denied. Lifted in Phase 5.
- **UI:** menu row (FolderOpen) under "Add files"; `FolderReferenceChip` row above the attachment gallery (tooltip = full path; ✕ soft-remove; aria-label + app Tooltip); hydrated via `list_conversation_folder_references`; TanStack invalidation, **no new event**.

### D7 — Manual editing of drafts

- Extend `PersonaService::update_draft` with optional `expected_content_hash` CAS (no parallel method — the D8 chokepoint stays closed). New Tauri command `update_persona_draft`; conflict ⇒ typed error + "reload draft" UI.
- `PersonaEditor`: drafts editable; "Open builder conversation ↗" via `builderDraftId`.
- Seeded drafts: manual edits allowed; `SOURCE_CHANGED_SINCE_SEED` unaffected.
- Accepted risk: draft edits between Phase 2 and Phase 6 have no artifact history (status quo).

### D8 — Versioned persona artifacts (Persona tab)

- `ArtifactType::Persona` (Rust + TS) + dedicated **`persona-library`** bucket (prd-library's policy excludes personas). Alternative (reuse Specification) rejected: type/bucket leakage into spec/plan listings.
- Migration: `personas.artifact_id TEXT NULL` (tip pointer); backfill one artifact per persona with content, stamped `persona_version = row.version`, `created_by_kind = backfill`.
- **Metadata mechanics (found gap):** both sync artifact helpers serialize `metadata_json` from `team_metadata` only — Phase 6.1 extends `ArtifactMetadata`/helpers (or uses a dedicated raw-SQL insert path) so appends can stamp `{persona_version, created_by}`. `persona.version` stays user-facing; chain `version` is internal. Version-history API extended with `created_by` + metadata.
- **Chokepoint = `PersonaService`, all six writers (verified complete, 2.1):** each content mutation appends in the same transaction (async→sync-in-transaction conversions for `update_draft`/`update_persona`/`approve_persona`; the two raw-SQL approval transactions gain the append inside their existing `run_transaction` closures — sync helpers join cleanly). Artifact failure aborts the mutation.
  - `approve_persona`: exactly one append even though it internally re-writes content before `set_status` (hook at the service layer).
  - **`approve_persona_as_new`**: when the slug changes, content is recomposed — that recompose is itself an append (`created_by: system`) inside the same transaction, so `personas.content` never diverges from the tip.
- **Graft-on-apply (decided — chains are strictly linear, a true graft is impossible):** on `apply_seeded_draft`, one artifact is appended to the **source persona's chain** (parent = source tip) carrying the draft's final content, with metadata `{source_draft_id, draft_tip_artifact_id, created_by: agent}`; the source's `artifact_id` repoints in the same transaction. The draft's interim chain becomes **orphaned rows** — intentionally: not shown in the source's history dropdown (history walks backward from the tip), but forensically recoverable via the graft metadata. Note for future work: `get_by_bucket`/`get_by_type` return every non-archived row — a future bucket browser over `persona-library` would show all chain versions, orphaned or not (no UI consumes these today). (UX consequence, stated: after a seeded apply, the Persona tab's dropdown shows the source lineage + the graft node; the draft's interim v1..vN iterations are no longer listed. Plain approvals keep their full chain — the draft chain *is* the persona chain.)
- **Draft hard-delete:** `hard_delete_draft`/`delete_bound_draft` also delete the draft's chain rows (orphan prevention).
- **Binding across approval — made uniform:** new column `chat_conversations.builder_result_persona_id`. All three approval paths, in-transaction: **clear `builder_draft_id`** (plain approve today leaves it set — that asymmetry is removed) and set `builder_result_persona_id` (plain: draft id, now active; seeded apply: source id; approve-as-new: new id). **M3 backfill for the pre-existing backlog:** every v1 plain-approved build left `builder_draft_id` pointing at a now-ACTIVE persona — the migration moves those ids to `builder_result_persona_id` and clears `builder_draft_id` wherever the bound persona is not a draft (else the Persona tab's "draft view" rule and the post-approval rejection predicate both misfire on legacy conversations).
  - Persona tab resolution: `builder_draft_id` set ⇒ draft view; else `builder_result_persona_id` set ⇒ approved view (read-only content of the result persona; actions Open in Settings / Refine with Agent). **Archived result persona** ⇒ tab renders the archived state (badge, refine disabled — `is_bindable` is active-only); the pointer is intentionally not cleared by `archive_persona`.
  - Post-approval `save_persona_draft`: rejected (typed "persona already approved — start a refine build") when `builder_draft_id IS NULL AND builder_result_persona_id IS NOT NULL`. With the uniform clearing above, this predicate now fires on **all three** paths.
- **`save_persona_draft` fail-closed (Phase 0.4):** caller-session identity required; the fail-open fall-through covers three states to close — (a) missing header, (b) header→conversation not found, (c) header→conversation not builder-mode (today (b)/(c) silently degrade via the `.filter()`). **Prerequisite in the same phase:** resume-path runtime contexts carry `conversation_id = None` today (`chat_service_context.rs:244,532,3558`) and the Codex lane substitutes `context_id` (the project id) — so a queued/resumed legacy builder would either send no header or the wrong id and be rejected. Phase 0.4 therefore first threads the real conversation id through all runtime-context construction sites (it folds into 0.1's single-seam work) and only then enables rejection. Trust boundary stated: the header is unauthenticated loopback HTTP (2.6) — spoofing from other local processes is out of scope here.
- **Persona tab** in `AgentsArtifactPane` (mode-gated; AUTOMATION_TAB precedent): content, scope badge, status pill, version dropdown with attribution, Approve / Approve-as-new / Open in Settings; skeleton-first. Standalone builder conversations depend on Phase 4b pane rendering.
- Events: `persona:draft_updated` += `artifact_id` (additive).

### D9 — Filesystem enforcement model

- **Per-conversation flag:** `McpRuntimeContext.enforce_filesystem_roots: bool` → **CLI arg only** (never process env — the env-fallback in `hydrateRalphxRuntimeEnvFromCli` would leak a process-env value into every MCP server the CLI spawns) → `RALPHX_FILESYSTEM_ENFORCED=1`.
- **Single derivation seam:** `build_mcp_runtime_context` has 7 construction sites and today doesn't receive `agent_mode`; the flag (and roots) must be computed in **one** place — thread `effective_mode`/context into `build_mcp_runtime_context` and derive there. Any per-call-site derivation fails open on the missed site (the `Default` derive makes `false` the silent default). Tests must cover fresh/resume/queued/recovery spawn shapes.
- Set **only** for `persona_builder`-mode and `Standalone`-context conversations.
- **Enforced mode:** allowed roots = configured roots exactly (CWD NOT implicit); empty ⇒ deny all; containment on realpath'd targets (symlink-inside-root escape rejected). **Unenforced mode: byte-identical behavior** (no clamping of the 15 fs_*-granted agents).
- **Builder read-root resolution (restructured — the current arm cannot express D4):**
  1. PersonaBuilder mode arm: if a **live on-disk ingest store** exists ⇒ `[ingest_root, private_workspace] + live folder refs` (legacy discriminator — the only observable one; old/new builds are DB-identical; workspace + refs are included so Phase-5 features — attach-time materialization, folder chips — don't produce hints pointing outside the enforced roots on legacy conversations). Otherwise **fall through** (no early-return with `[]`).
  2. Fall-through resolves per context: Project build ⇒ `[project working_directory, private workspace] + folder refs`; Standalone ⇒ `[private workspace] + folder refs`. The `project_path == CWD` dedup (:1694-1696) is **bypassed for enforced conversations** — it assumes CWD is implicitly readable, which enforced mode explicitly revokes.
  3. Standalone-context arm and mode arm precedence: mode arm first (legacy check), then context arm — standalone conversations can never have a legacy ingest store, so this is unambiguous.
- Phase 0 sets the flag for existing builder conversations (roots = ingest root when live; **refine builds with no ingest store become deny-all on fs tools by design** — see D1 exception).
- Delegated/child agents: `Delegation`-context conversations are unenforced by the rule above; the escape hatch is closed structurally — the extractor has no delegation rights (fail-closed allowlist), and standalone callers are rejected from `delegate_start` (D3.8).
- Claude Task subagents share the parent CLI's MCP server ⇒ inherit enforcement correctly by construction (and the extractor has no Task tool).
- Chat-mode standalone: MCP `fs_*` enforced; Claude native reads cross its permission-prompt boundary, while Codex runs `workspace-write`/`on-request` and fails closed when non-interactive execution would require fresh approval (honest statement in D3).

---

## 5. UX Specification

Accent `#ff6b35`, SF Pro, existing tokens; icon-only buttons get `aria-label` + app Tooltip (FolderReferenceChip ✕, UX-1 `[⋯]`, scope badges). Shells paint before data hydrates; Persona tab renders a skeleton before artifact fetch. No inline-style gradients or chained canvas vars (WKWebView rules).

### UX-1 — Settings → Personas (scoped list + editable drafts)

```
┌─ Settings ▸ Personas ──────────────────────────────────────────────┐
│  Personas                                    [＋ New] [🤖 Build with Agent] │
│                                                                    │
│  Scope: [ All ▾ ]   (All | Global | ProjectName…)                  │
│                                                                    │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │ ● Code Reviewer        Global      active   v4    [Edit] [⋯] │  │
│  │ ● Support Voice        ▣ ralphx    active   v2    [Edit] [⋯] │  │
│  │ ◐ Docs Writer (draft)  ▣ ralphx    draft    v3    [Edit] [⋯] │  │
│  │      └─ built by agent · Open builder conversation ↗          │  │
│  └──────────────────────────────────────────────────────────────┘  │
│  [⋯] = Refine with Agent / Approve / Archive / Delete draft        │
└────────────────────────────────────────────────────────────────────┘
```

- Scope column: `Global` badge or project badge (▣); filter defaults to All (`PersonaScopeFilter::All`).
- Drafts fully editable + "Open builder conversation ↗" when bound.
- Editor: read-only Scope row (fixed at creation) + Version history link (Phase 6 components).

### UX-1b — Build with Agent: scope chooser (dialog in Settings)

```
┌─ Build persona with agent ───────────────────────────┐
│  Where should this persona be available?             │
│                                                      │
│  (•) Global — all projects                           │
│      Runs in a private workspace. Attach files or    │
│      folders in the chat to give the agent context.  │
│                                                      │
│  ( ) Project: [ Select project… ▾ ]                  │
│      The agent may also analyze this project's code  │
│      and docs for persona-relevant signals.          │
│                                                      │
│                              [Cancel]  [Start build] │
└──────────────────────────────────────────────────────┘
```

"Start build" → close Settings modal → Agents view, start composer prefilled + project-locked (UX-3). **Refine with Agent skips this dialog** (scope = source persona's scope).

### UX-2 — Agents new-conversation screen: projectless option

```
┌─ Start a conversation ─────────────────────────────────────────────┐
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  Message…                                                    │  │
│  │                                                              │  │
│  │  [＋] [◎]                                       [Mode: Chat ▾]│  │
│  └──────────────────────────────────────────────────────────────┘  │
│   Project: [ ▣ ralphx ▾ ]   Base: [ main ▾ ]        ← project line │
│            └─ dropdown: ───────────────────┐                       │
│               │  ▣ ralphx                  │                       │
│               │  ▣ other-project           │                       │
│               │  ─────────────             │                       │
│               │  ⌀ No project (standalone) │                       │
│               └────────────────────────────┘                       │
│   When "No project": Base picker hidden; Team toggle hidden;       │
│   persona picker [◎] hidden; modes limited to Chat · Persona;      │
│   caption: "Runs in a private workspace";                          │
│   composer assist (/, @plan, @path, skills) unavailable (v1)       │
└────────────────────────────────────────────────────────────────────┘
```

- "No project" lives inside the project picker; picker enabled at zero projects.
- `[◎]` = existing icon-only `PersonaPickerControl` (CircleDot + tooltip), not a labeled chip.
- Project-requiring modes snap back with an inline note when "No project" is active.

### UX-3 — Persona build start (prefilled from Settings, or picked manually)

```
┌─ Start a conversation ─────────────────────────────────────────────┐
│  ╭ Persona build ─────────────────────────────────────────────╮    │
│  │ 🎭 Building a Global persona · private workspace           │    │
│  │    Attach files/folders below, or just describe the persona│    │
│  ╰────────────────────────────────────────────────────────────╯    │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  Describe the persona you want, or attach context…          │  │
│  │  [＋]                                        [Mode: Persona ▾]│  │
│  └──────────────────────────────────────────────────────────────┘  │
│   Project: [ ⌀ No project ▾ ] 🔒     (locked when opened from      │
│                                       Settings scope chooser)      │
└────────────────────────────────────────────────────────────────────┘
```

- Manual path identical to Settings entry. Refine variant: "Refining 'Support Voice'".
- Team toggle hidden for Persona mode (any context).

### UX-4 — Builder conversation: interview loop

```
┌─ Conversation (Persona) ──────────────────┬─ Artifacts ─ [Persona] ┐
│ user: Build a support-agent persona from  │  Persona draft  v3 ▾   │
│       these docs. 📁 support-playbook     │  ────────────────────  │
│                                           │  name: support-voice   │
│ agent: Analyzed 14 files. Key signals:    │  Scope: Global         │
│        empathetic tone, no-blame language │                        │
│ ┌─ ? Question from agent ──── 1 of 3 ───┐ │  ## Voice              │
│ │ Which audience should this persona     │ │  Empathetic, direct…   │
│ │ optimize for?                          │ │                        │
│ │  (1) End customers   (2) Internal eng  │ │  v1 agent · v2 agent  │
│ │  (3) Both            [Skip]            │ │  v3 you (manual edit) │
│ └────────────────────────────────────────┘ │                        │
│ ┌──────────────────────────────────────┐   │  [Approve persona]     │
│ │ 2                                    │   │  [Open in Settings]    │
│ └──────────────────────────────────────┘   │                        │
└───────────────────────────────────────────┴────────────────────────┘
```

Interview = existing `QuestionInputBanner`. Right pane = Persona artifact tab (Phase 6). After a **seeded** approve, the dropdown shows the source lineage + graft node (interim draft iterations no longer listed — D8); plain approvals keep their full chain.

### UX-5 — Composer "+" menu & folder chips

```
   [＋]──┐                       Composer with references:
   ┌─────┴──────────────┐       ┌────────────────────────────────────┐
   │ 📎 Add files       │       │ 📁 design-notes ✕   📁 brand-kit ✕ │ ← folder chips (sticky)
   │ 📁 Add folder      │  →    │ ┌──────────────────────────────┐   │
   │ ────────────────   │       │ │ img.png 24KB ✕               │   │ ← file attachments (per-turn)
   │ (existing items…)  │       │ └──────────────────────────────┘   │
   └────────────────────┘       │  Message…                    [Send]│
                                └────────────────────────────────────┘
```

- Chip tooltip = absolute path; persists across turns/reloads; ✕ soft-removes.
- Start composer: draft-local until the seeded conversation exists.
- Hidden for `persona_builder` conversations until Phase 5 (D6 cross-phase seam).
- Folders are references everywhere — no copy, no toast; the chip is the whole story.

### UX-6 — Persona artifact tab

- Mode-gated (`persona_builder` only). Header: name, scope badge, status pill, version dropdown (created-by + timestamp; historical versions read-only).
- States: **draft** (bound draft + Approve / Approve-as-new for seeded), **approved** (read-only result persona via `builder_result_persona_id`; Open in Settings / Refine with Agent), **archived result** (badge, refine disabled), **empty** ("The agent will draft the persona here after its first pass").

---

## 6. User Stories & Flows

### US-1 — Build a global persona from local folders (no project)

```
Settings▸Personas         Agents view                        Backend
     │                        │                                 │
 [Build with Agent]           │                                 │
     │ scope chooser: Global  │                                 │
     ├─ setStartConversationDraft(mode=persona_builder,         │
     │    projectId=null, projectLocked)                        │
     ├─ closeModal · setFocusedProject(null) · clearSelection   │
     ├─ setCurrentView("agents") ─▶ start composer prefilled    │
     │                        │  "No project" 🔒 (lock lifted   │
     │                        │   into composer state)          │
     │                        │ user: ＋ Add folder ~/notes      │
     │                        │   (draft-local; seeds standalone│
     │                        │    conversation, registers ref) │
     │                        │ types goal, Send ──────────────▶ start():
     │                        │                                 │  ensure private ws
     │                        │                                 │  spawn extractor (selected Claude/Codex lane)
     │                        │                                 │   cwd=ws, roots=[ws, ~/notes],
     │                        │                                 │   ENFORCED (read live via fs_*)
     │                        │ ◀─ analysis summary ────────────┤
     │                        │ ◀─ ask_user_question (1 of 3) ──┤
     │                        │ user answers chips ────────────▶│
     │                        │ ◀─ save_persona_draft ──────────┤ draft row (global) + binding
     │                        │    Persona tab shows v1         │ + artifact v1
     │                        │ …iterate… v2, v3                │
     │                        │ [Approve persona] ─────────────▶ approve: draft→active (global)
     │                        │    tab → approved state         │ builder_draft_id cleared,
     │                        │                                 │ builder_result_persona_id set
```

### US-2 — Build a project persona that mines the repo

```
Build with Agent ▸ Project: ralphx
  → start composer: Project=ralphx 🔒, Mode=Persona
  → Send "Create a reviewer persona matching our review culture"
  → Conversation: Project context, mode=persona_builder
      cwd = ralphx wd; private ws created too;
      roots = [ralphx wd, ws] ENFORCED (CWD-dedup bypassed) + folder refs
  → Agent samples docs/reviews, interviews (≤3 rounds), saves draft
  → Draft project_id = ralphx → only offered in ralphx pickers
  → Approve in Persona tab → active, scoped to ralphx
```

### US-3 — Add an outside folder to a normal Edit conversation

```
Edit conversation on ralphx
  ＋ ▸ Add folder ▸ ~/Design/brand-kit (outside repo)
  → chip appears; row created (realpath'd, validated)
  → next send: read root added (informational; conversation UNENFORCED);
    system prompt gains escaped <referenced_folders> hint
  → agent Reads brand-kit without permission dialogs
    (read-tools-only predicate; Bash unaffected)
  → ✕ → next spawn: root + hint gone
```

### US-4 — Manually fix an agent-generated draft

```
Settings▸Personas ▸ "Docs Writer (draft)" ▸ Edit
  → Save (carries expected_content_hash)
  → conflict? → "Draft changed — reload"
  → success: version bumps, artifact appended (created_by=user),
    persona:draft_updated refreshes builder surfaces
```

### US-5 — Projectless quick chat

```
Agents ▸ start composer ▸ "No project" ▸ Mode: Chat
  → attach file? → seeds standalone conversation, uploads against it
  → Send → Standalone conversation, private workspace as cwd;
    MCP fs_* ENFORCED to [ws]; Claude native reads cross the permission-prompt
    boundary, while Codex approval-requiring actions fail closed non-interactively
  → sidebar "No project" group; composer assist disabled (v1)
  → app restart mid-run → recovered (chat_resumption enumerates Standalone)
```

---

## 7. Data Model & API Changes (summary)

### Migrations

| # | Change |
|---|---|
| M1 | `personas.project_id TEXT NULL` (+ index); scoped active-slug unique index replaces `idx_personas_slug_live`; update `map_live_slug_unique_error` |
| M2 | `conversation_folder_references(id, conversation_id, folder_path, display_name, created_at, removed_at)` + index |
| M3 | `chat_conversations.builder_result_persona_id TEXT NULL` (+ index); backfill: non-draft `builder_draft_id` bindings move to `builder_result_persona_id` and are cleared |
| M4 | `personas.artifact_id TEXT NULL`; `ArtifactType::Persona`; seed `persona-library` bucket; `ArtifactMetadata`/helper extension for persona metadata; backfill artifacts stamped `persona_version` |

### New/changed commands & endpoints

| Surface | Change |
|---|---|
| Tauri | `update_persona_draft` (CAS), `add/remove/list_conversation_folder_reference(s)` |
| Tauri | `start_agent_conversation`: `project_id: Option`; `source_persona_id` (persona builds; scope-match enforced); standalone Team + Persona-mode Team rejection |
| Tauri | `create_agent_conversation`: standalone arm (`context_id` optional; backend self-keys; `new_standalone()`) |
| Tauri | `list_personas(scope: All \| GlobalOnly \| GlobalAndProject)`; `create_persona_draft` accepts `project_id`; standalone sidebar enumeration |
| Tauri | `get_artifact_version_history`: += `created_by`, metadata |
| Backend | `delegate_start`: typed rejection for standalone callers |
| Removed (Phase 7) | `ingest_persona_context`, `get_persona_builder_ingest_status`, `create_persona_builder_conversation` (seeding internals moved first) |
| Removed (Phase 5) | `ensure_persona_builder_has_live_context` + queue `builder_context_error` (unconditional gate retirement) |
| HTTP/MCP | `save_persona_draft`: caller-session required (3-state fail-closed); post-approval typed rejection; in-transaction artifact append |
| MCP server | enforced-mode `fs_*` containment (flag via CLI arg, realpath'd); read-tools-only trusted-roots predicate; **rebuild both servers** |

### Events

- `persona:draft_updated` += `artifact_id` (additive). No new events.

---

## 8. Security & Path Safety (consolidated)

1. **Folder references:** realpath-canonicalize → `validate_absolute_non_root_path` → reject symlink roots → reject roots under `app_data_dir` → store canonical; re-validate at every sink; XML-escape at prompt render; reject control chars in `display_name`.
2. **Private workspace:** hash-derived components; `require_under_root` on every write; only composer attachments are copied in (≤ composer caps; folders are never copied — enforced live references per D4); crash-orphan sweep deletes only under the validated app-owned root.
3. **Enforced mode (D9):** roots-only (no implicit CWD), empty ⇒ deny, realpath before containment; single derivation seam covering all 7 runtime-context construction sites; flag rides CLI args only. Tests: `../`, absolute escape, symlink escape, CWD-read rejection, empty-roots denial, accepted path, fresh/resume/queued/recovery parity, refine-build (no ingest) deny-all fixture.
4. **Permission bridge:** read-tools-only predicate (realpath'd); Bash branch untouched; folder refs never grant write/Bash.
5. **`save_persona_draft`:** caller identity required (3-state fail-closed); binding-scoped writes; post-approval rejection. Trust boundary: unauthenticated loopback header (pre-existing posture, documented).
6. **Standalone CWD fail-closed:** missing workspace ⇒ typed spawn error.
7. **Persona resolver:** typed repo errors abort; scope check on every persona-returning branch incl. `Explicit`; suppression writes attribution.
8. **Codex lane:** Standalone Chat is supported with per-launch `on-request`/`workspace-write` containment; global PersonaBuilder is supported under the extractor's existing MCP compatibility contract (`never`/`danger-full-access`) plus enforced filesystem roots. Delegation: extractor has none.

---

## 9. Phase-by-Phase Implementation Plan

Independently landable slices. Every phase: TDD, zero new warnings, dual clippy gates + `scripts/test-rust-fast.sh pr`, Vitest, MCP server rebuilds where touched.

### Phase 0 — Filesystem enforcement + persona write hardening (D9)  `flags: none (hardening)`
| Task | Detail |
|---|---|
| 0.1 | `enforce_filesystem_roots` on `McpRuntimeContext`; **single derivation seam** (thread `effective_mode` into `build_mcp_runtime_context`; all 7 construction sites covered); CLI arg only (never process env); set for `persona_builder` conversations (roots = live ingest root; none ⇒ empty ⇒ deny) |
| 0.2 | MCP server: enforced-mode containment in `fs_*` (roots-only, no implicit CWD, empty ⇒ deny, realpath before containment); unenforced mode byte-identical |
| 0.3 | Permission bridge: read-tools-only `isTrustedReadRootPath` (realpath'd); Bash branch untouched |
| 0.4 | Thread the real `conversation_id` through all runtime-context construction sites (resume paths carry `None` today at `chat_service_context.rs:244,532,3558`; Codex substitutes `context_id`) — then `save_persona_draft` 3-state fail-closed (missing header / conversation not found / not builder-mode); remove client-`draft_id` fall-through |
| 0.5 | Extractor prompt touch-up (both harness files): fs-first workflow reworded so a roots-empty refine build leads with the interview + draft tools instead of opening on denied `fs_*` calls (full rewrite still lands in Phase 5.5) |
| 0.6 | Tests: full escape matrix; **refine-build (bound draft, no ingest) ⇒ deny-all fs, interview/draft tools work**; fresh/resume/queued/recovery spawn parity incl. conversation-id header presence; unenforced agents byte-identical (worktree + ideation fixtures); Bash auto-approval unchanged; save-draft header matrix |
| Exit | Builder conversations reject reads outside configured roots **including CWD**; legacy refine builds intentionally lose the implicit-CWD project read (documented behavior change); queued/resumed legacy builders still save drafts; zero change for unenforced agents |

### Phase 1 — Persona project scoping (D2)  `flag: agent_personas`
| Task | Detail |
|---|---|
| 1.1 | M1 + `Persona.project_id` + `PersonaScopeFilter` API + scoped slug checks in `personas/mod.rs` **and** `persona_update_approval.rs`; update `map_live_slug_unique_error` |
| 1.2 | Shared bindability predicate (bind/start/send incl. `Explicit`) + `project_scope_mismatch` attribution |
| 1.3 | App-level project-deletion sweep (personas + all three binding columns) |
| 1.4 | Settings scope filter/badges; create-form scope select; `usePersonas`/`PersonaPickerControl` scope param + grouping |
| 1.5 | Tests: scope filtering (all three filter shapes); cross-project bind rejection; Explicit-branch suppression + attribution; approval-module slug coexistence; slug-conflict error mapping; migration default-global; delete-project sweep |
| Exit | Global/project personas functional; existing personas unaffected |

### Phase 2 — Manual draft editing (D7)  `flag: agent_personas`
| Task | Detail |
|---|---|
| 2.1 | `update_draft` CAS extension + `update_persona_draft` command |
| 2.2 | `PersonaEditor` draft editing + conflict-reload + builder-conversation link |
| 2.3 | Tests: happy path; CAS conflict; seeded-draft edit + `SOURCE_CHANGED_SINCE_SEED`; events |
| Exit | Any persona editable. (Accepted: no history until Phase 6) |

### Phase 3 — Composer Add Folder (D6)  `always on`
| Task | Detail |
|---|---|
| 3.1 | M2 + repo + 3 commands + validation chain (§8.1) |
| 3.2 | Ordered overlays; `<referenced_folders>` with escaping; all resolver/spawn/recovery paths; Codex parity + `folder_refs_skipped` logging |
| 3.3 | Read-root wiring for Project conversations (after existing arms; survives early-returns) |
| 3.4 | UI: menu row (hidden for `persona_builder` until Phase 5), `FolderReferenceChip`, draft-local pre-send + seeded registration, hydration, remove, invalidation |
| 3.5 | Tests: validation matrix; overlay on fresh/resume/queued/recovery; escaping; chip persistence; builder-mode hiding; first-paint discipline |
| Exit | Project conversations reference outside folders; reads frictionless; builder conversations unaffected |

### Phase 4 — Standalone conversations (D3)  `flag: standalone_conversations`
**4a Backend**
| Task | Detail |
|---|---|
| 4a.1 | Enum variant + compiler sweep + **named-site checklist**: `message_queue.rs:581-590` (exhaustive dispatch conversion), `chat_service/mod.rs:2677-2709`, `agent_conversation_archive.rs:189`, `chat_resumption.rs:252/274/336/438`, `chat_service_handlers.rs:64-70`, `chat_service_streaming.rs:78-86`, full `ChatContextType::Project`/`"project"` audit |
| 4a.2 | `standalone_workspace.rs` (also serves persona builds; idempotent create; archive-safe; **crash-orphan sweep only**) |
| 4a.3 | CWD + read-root arms (fail-closed); enforcement flag for standalone |
| 4a.4 | `create_agent_conversation` standalone arm (`context_id` optional; `new_standalone()`); `StartAgentConversationInput.project_id: Option` + mode allowlist (`chat`) + Team rejection + seeded-ownership rule; `delegate_start` standalone rejection |
| 4a.5 | Sidebar enumeration (evaluate `list_recent_resumable_by_context_type` reuse) |
**4b Frontend**
| Task | Detail |
|---|---|
| 4b.1 | `chat-context-registry.ts` standalone entry (full checklist) |
| 4b.2 | **`useStartAgentConversation.ts` standalone-aware** (optimistic conversation, seeded creation, store keys, startInput) |
| 4b.3 | Main region + selection model project-less support |
| 4b.4 | `IntegratedChatPanel.projectId: string \| null` + hook short-circuits; `AgentComposerSurface.project.value: string \| null` + assist disable + "No project" rendering |
| 4b.5 | Start composer: "No project" item (zero-project reachable); `requiresProject` mode metadata + gating; captions; Team + persona picker hidden; `AgentStartConversationDraft.projectId: string \| null` |
| 4b.6 | Sidebar "No project" group + keys/invalidation/running-state |
| 4b.7 | Tests: start/spawn/CWD/roots; fail-closed missing workspace; archive/restore incl. running-agent stop via real context; **restart recovery**; provider-pause requeue; queue-count visibility; agent-waiting; session auto-titling; mode/Team/delegation rejection matrix; selection/rendering; sidebar group |
| Exit | Projectless Chat ships end-to-end (visible, selectable, archivable, restorable, recoverable) |

### Phase 5 — Persona build flow v2 (D4, D5)  `flag: agent_personas` (old builder path still live)
| Task | Detail |
|---|---|
| 5.1 | Mode-rejection lifts per the lift/keep table; "Persona" in both mode lists + locked-menu fix (all options disabled for `automation`/`persona_builder`); standalone mode allowlist += `persona_builder`; Persona-mode Team rejection |
| 5.2 | Start pipeline: `source_persona_id` → seeded bound draft at creation (seeding internals → `PersonaService`; `SavePersonaDraftInput` scope field); scope stamped from conversation; **refine scope lock** (deep link skips chooser; service rejects scope mismatch) |
| 5.3 | Workspace for **every** builder conversation (idempotent, any context); attachment sync (start-time + attach-time; text-only, binary rejection at attach; content inlining suppressed in builder mode → workspace path references + `fs_read_file`); folder refs as live enforced roots for builders (no copying); builder read-root restructure per D9 (legacy fall-through, CWD-dedup bypass under enforcement); folder-ref "+ Add folder" un-hidden for builders; Codex global builds retain MCP-compatible launch policy plus enforced filesystem roots |
| 5.4 | Settings deep link (scope chooser; `projectLocked` lifted into composer state; closeModal + setFocusedProject + clearSelection); "Refine with Agent" |
| 5.5 | Extractor prompt rewrite (analyze → interview ≤3 → draft), both harness files; **unconditional ingest-gate retirement** (send call sites + queue `builder_context_error`) |
| 5.6 | Tests: start-to-draft both scopes; enforced isolation (fs_* denial of non-attached paths; attached-folder reads succeed live); binary-attachment rejection at attach time + text-attachment workspace reachability + no-inline in builder mode; scope stamping; refine provenance + scope-lock rejection; redirect draft consumption + lock survives projects-query churn; attachment-sync idempotency; **queued follow-up on a standalone builder resumes with extractor identity/mode/draft/workspace intact**; legacy builder conversation still resumes with ingest+workspace roots |
| Exit | Both build kinds run through the Agents view with interview UX; old Settings path untouched |

### Phase 6 — Versioned persona artifacts + Persona tab (D8)  `flag: agent_personas`
| Task | Detail |
|---|---|
| 6.1 | M3 + M4; `ArtifactType::Persona` (Rust + TS) + `persona-library` bucket; **`ArtifactMetadata`/sync-helper extension for `{persona_version, created_by}`**; backfill |
| 6.2 | Chokepoint: all six writers append in-transaction (async→sync conversions; raw-SQL transaction joins; approve single-append; **approve-as-new recompose append**; **graft-on-apply per D8** with metadata; draft hard-delete removes chain rows) |
| 6.3 | Uniform binding transitions: all three approvals clear `builder_draft_id` + set `builder_result_persona_id` in-transaction; post-approval `save_persona_draft` typed rejection |
| 6.4 | Version-history API += `created_by`/metadata; Persona tab (draft/approved/archived-result/empty states; skeleton-first); Settings Version-history link |
| 6.5 | Tests: append per writer with attribution; atomicity; single-append on approve; graft shape (source chain + orphaned interim + metadata recoverability); recompose append; backfill version stamping; binding uniformity across all three approvals; archived-result state; post-approval rejection; history UI |
| Exit | Full persona history; approval keeps the tab alive on every path |

### Phase 7 — Cutover & legacy (D5)  `flag: agent_personas`
| Task | Detail |
|---|---|
| 7.1 | Settings "Build with Agent" → scope chooser + deep link exclusively; delete `PersonaBuilderView` gate/picker + `ingest_persona_context` + `get_persona_builder_ingest_status` + `create_persona_builder_conversation` |
| 7.2 | Deletion checklist: `usePersonas.ts` ingest hooks (+ tests), `personas-flag-off.test.tsx` import, `PersonasSection.showBuilderEntry`, `types/persona.ts` ingest schemas, `commands/mod.rs`/`registry.rs` registrations, `persona_ingest_tests.rs` |
| 7.3 | Legacy compat: the D9 fall-through keeps live ingest stores working for old conversations indefinitely; **ingest stores are NOT swept** (age-based reclamation deferred with the workspace sweep) |
| 7.4 | Docs: `docs/features/agent-personas.md`, `.claude/rules` agent-mcp-tools/agent-type-map deltas |
| Exit | One builder path; no orphaned UI/commands; legacy conversations resumable indefinitely |

**Dependencies:** 0 → {3, 5}; 1 → 5; **3 → 5**; 4 → 5; **5 → 6** (landing 6's binding-clearing before 5.5's gate retirement would make ingest-era conversations sendable post-approval through the still-live gate's fall-through); {5, 6} → 7; 2 independent after 1. Order: 0, 1, 2, 3, 4, 5, 6, 7.

---

## 10. Non-Goals (v1)

- Scope change after creation; slug rename; multi-persona per conversation.
- Persona binding/injection for Standalone or non-Project contexts.
- Personas for teammates/pipeline agents; diff view between versions.
- Filesystem enforcement for agents other than persona-builder/standalone; changing provider/lane security defaults outside the Standalone Chat launch class.
- Composer assist in standalone conversations.
- Folder references in standalone **chat** (persona builds support them in any context); folder refs granting write/Bash.
- Changing generic (non-builder) attachment inlining behavior.
- Standalone for `edit/plan/ideation/review_pr/automation`; delegation from standalone conversations.
- Age-based workspace/ingest reclamation; reclamation UI.
- Fixing the unauthenticated-loopback trust boundary (pre-existing posture).

## 11. Open Questions — assumed defaults (pending owner confirmation)

1. **Interview bound** → **prompt-enforced ≤3 rounds, no backend counter.** The seam already has a wait timeout + Skip; misbehavior is a prompt fix. A backend cap is deferred machinery.
2. ~~Folder-tree materializer caps~~ — **resolved moot (owner, 2026-07-16): folders are never copied.** Folder references are live enforced read roots read on demand via `fs_*` (D4); the only copy is composer attachments into the workspace, already bounded by composer caps. The v1 ingest caps survive only in the legacy store until Phase 7.
3. **Standalone architecture** → **honest `ChatContextType::Standalone`, Phase 4 cost accepted.** Projectless conversations are an explicit user-facing requirement, not plumbing; a scratch project would leak into every project-scoped surface permanently.
4. ~~Workspace reclamation window~~ — resolved: crash-orphan cleanup only; age-based deferred.
5. **Post-approval builder conversation** → **typed rejection (as spec'd).** Auto-seeding a refine draft from a stray post-approval write is a silent side effect (false-success pattern); Refine is one explicit click in the same tab.

## 12. Verification Checklist (pre-merge, stateful-workflow-review rule)

- Persona resolution: scope check on every persona-returning branch (incl. `Explicit`); typed repo errors abort; suppression writes attribution.
- Enforcement: single derivation seam (all 7 runtime-context sites); empty-roots deny; CWD-read rejection; realpath containment; unenforced byte-identical; fresh/resume/queued/recovery parity.
- Standalone: missing workspace ⇒ typed error; archive stop uses real context; restart recovery; provider-pause requeue; queue counts; agent-waiting.
- Artifact chain: single chokepoint (six writers); in-transaction appends; approve single-append; approve-as-new recompose append; graft shape + metadata; no divergence between `personas.content` and tip; draft-delete chain cleanup.
- Bindings: uniform across all three approvals; post-approval and cross-conversation `save_persona_draft` rejected; caller identity 3-state fail-closed.
- Folder overlay on fresh/resume/queued/recovery; escaping; Codex skip logged; builder-mode hiding until Phase 5.
- MCP servers rebuilt; `mcp_tools` lists unchanged for unrelated agents.
