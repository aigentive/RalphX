> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# Project Creation

The Create New Project dialog opens on an explicit intent chooser instead of a single folder picker. Each intent knows what it is about to do and says so in plain language — no git commands, ref syntax, or error codes are ever rendered on screen.

## Intents

| Intent | What RalphX does | Key commands |
|---|---|---|
| Clone Repository | Downloads a remote repository into a folder you pick, then registers it | `validate_clone_target` → `start_project_clone` → `create_project` |
| Create New Repository | Makes an empty folder, starts version history in it, then registers it | `prepare_new_project_directory` → `create_project` |
| Add Existing Repository | Inspects a folder you already have and registers it as-is | `inspect_project_candidate` → `create_project` |

`create_project` stays the single registration path for all three. `github_pr_enabled` remains derived there from live repository capability; the dialog only displays it.

## Preflight probe

`inspect_project_candidate(path)` is read-only and never writes. It answers one tagged verdict (`kind`, snake_case):

| Verdict | Meaning | Dialog recovery |
|---|---|---|
| `not_found` / `not_a_directory` | Nothing usable at that path | Ask for a different folder |
| `empty_directory` | Empty, outside any repository | Offer Create New instead |
| `non_empty_non_repo` (`entry_count`) | Has files, no version history | Switch to Create New carrying the path |
| `nested_in_repository` (`repository_root`) | Sits inside another repository | Offer the repository root |
| `detached_head` (`repository_root`) | Repository with no current branch | Blocked; not a usable project |
| `repository` | Usable root; carries branches, default branch, dirty flag, capability, `already_registered_as` | `already_registered_as` blocks duplicate registration and offers "Open X instead" |
| `inspection_failed` (`message`) | Probe itself failed | Degrades to "you can still continue" — never blocks |

Duplicate detection compares **canonicalized** paths, so a case-different or trailing-slash path cannot register the same repository twice on macOS.

`prepare_new_project_directory(parent, name)` is the only writing command in the probe family; it exists because `bootstrap_project_repository` refuses a directory that does not exist yet and the frontend must not create directories. It reports `created`, and `discard_prepared_project_directory` rolls back a `created: true` folder if `create_project` then fails — refusing to remove anything holding content RalphX did not create.

`validate_worktree_parent(path, repositoryRoot?)` expands `~` through the same helper execution uses, then answers `ok` | `not_found` | `not_a_directory` | `inside_repository` | `not_writable` | `invalid`. A parent inside the repository fails in the dialog rather than at first task execution. `not_writable` is a best-effort metadata read and advisory only.

## Clone lifecycle

```
validate_clone_target → start_project_clone → {clone_progress}* → clone_completed | clone_failed | clone_cancelled
                                            ↘ get_clone_job_status (reconciliation)
```

| Command | Behavior |
|---|---|
| `validate_clone_target` | Pure: normalizes the URL, derives the folder, checks the destination. Total shape — an unusable entry returns `ready: false` + `problem`, never an exception |
| `start_project_clone` | Returns `{ jobId }`. Deduplicates by destination, so a double-click joins the running clone |
| `cancel_project_clone` | `false` when the job already finished |
| `get_clone_job_status` | Live or **retained** terminal status; `unknown` for an expired or never-existent id |

### Why status is re-readable

`TauriEventBus` only bridges events that reach its JS callback. Anything emitted before the listener registers is lost outright, so a UI trusting events alone can spin forever on a finished clone. Terminal outcomes are therefore retained in `CloneJobRegistry` for `git.clone_timeout_secs` and re-readable by job id. `unknown` is explicitly terminal so the UI fails closed with a retry.

### URL shapes

| Input | Normalized | Folder |
|---|---|---|
| `https://host/o/r.git` \| `https://host/o/r` | unchanged | `r` |
| `git@host:o/r.git` \| `ssh://git@host/o/r.git` | unchanged | `r` |
| `o/r` | `https://github.com/o/r.git` | `r` |
| `https://github.com/o/r/tree/<branch>` | `https://github.com/o/r.git` + branch | `r` |
| `file://…`, `/abs`, `./rel`, bare host | rejected `CLONE_URL_INVALID` | — |

Local clones are out of scope: "this folder is already here" is Add Existing. Normalization lives at the command boundary (`git_service/clone_url.rs`), so `GitService::clone_repository` clones whatever URL it is handed.

### Failure codes

`CLONE_URL_INVALID` · `CLONE_DEST_INVALID` · `CLONE_DEST_NOT_EMPTY` · `CLONE_AUTH_FAILED` · `CLONE_NOT_FOUND` · `CLONE_NETWORK` · `CLONE_TIMEOUT` · `CLONE_CANCELLED` · `CLONE_UNKNOWN`

Classification order matters: `does not appear to be a git repository` → not-found first (git pairs it with a generic auth phrase), then auth, then not-found, then network. GitHub reports an invisible private repository as "Repository not found", which is a credential problem — so auth outranks the broad not-found patterns.

### Safety invariants

| Invariant | How |
|---|---|
| Never blocks workspace automation | Runs on `GitCommandLane::Clone`, its own permits — never the single-permit Background lane |
| Never blocks on a prompt | Clone-only `GIT_ASKPASS=""`, `SSH_ASKPASS=""`, `GIT_SSH_COMMAND="ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new"`. `GIT_TERMINAL_PROMPT=0` is already inherited and is not re-added |
| Never deletes user data | Ownership recorded before spawn: created-by-us → removed on failure; pre-existing-empty → emptied, never removed; non-empty → refused before spawn |
| Never silently retries | `spawn_streaming` bypasses the transient-error retry loop; a partial destination must not be re-attempted |
| Never floods the WebView | Progress coalesced to ≤1 event/100 ms; phase changes and the final update before a terminal event always get through |
| Never leaves a dangling job | Every exit (success/failure/timeout/cancel) records a terminal outcome **then** emits exactly one terminal event |

## Events

| Event | Payload |
|---|---|
| `project:clone_progress` | `{ jobId, phase, percent?, received?, total?, line }` |
| `project:clone_completed` | `{ jobId, state, destination, defaultBranch, capability }` |
| `project:clone_failed` | `{ jobId, state, code, message, cleanedUp }` |
| `project:clone_cancelled` | `{ jobId, state, cleanedUp }` |

`phase` ∈ `connecting` · `counting` · `compressing` · `receiving` · `resolving` · `checking_out`. `--recurse-submodules` replays the sequence per submodule, so the parser accepts repeats and percentage regressions.

## Config

| Key | Default | Env |
|---|---|---|
| `git.clone_timeout_secs` | 900 | `RALPHX_GIT_CLONE_TIMEOUT_SECS` |

Doubles as the retention window for terminal clone outcomes. Carries a serde default, so a `config/ralphx.yaml` written before the field existed still loads.

## Extras

| Surface | Behavior |
|---|---|
| Recent repositories | Frontend-only localStorage list under Add Existing; already-registered entries hidden; a stale entry degrades to the normal probe error card. No backend, no migration |
| GitHub repo picker | `list_github_repositories` (`gh repo list`, repo-less invocation, tolerates unknown JSON fields). Shown only when already authenticated; any failure falls back silently to URL entry |
| Advanced clone options | `depth` / `singleBranch` / `recurseSubmodules`, all defaulted off — defaults emit no extra flags |

## Scope

The clone job registry lives on the Tauri `AppState` only. It is deliberately **not** cloned into the HTTP/MCP object graph because no MCP caller exists — a recorded limitation, not an oversight.

## Files

`src-tauri/src/commands/project_probe_commands.rs` · `project_clone_commands.rs` · `src-tauri/src/application/clone_job_registry.rs` · `clone_job_runner.rs` · `src-tauri/src/application/git_service/{clone,clone_url,clone_progress}.rs` · `frontend/src/components/projects/ProjectCreationWizard/`
