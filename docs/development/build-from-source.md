# Build From Source

Use this path when you want to run RalphX from a checkout instead of installing the signed app.

## Requirements

- macOS 13+ (Ventura or later)
- Node.js 18+ and npm
- Rust via [rustup.rs](https://rustup.rs)
- Git

The repo pins its Rust toolchain in [rust-toolchain.toml](../../rust-toolchain.toml).

## First Run

```bash
git clone https://github.com/aigentive/ralphx.app.git ralphx.app
cd ralphx.app
cd frontend
npm install
npm run tauri dev
```

First build compiles the Rust backend. Subsequent starts are faster.

## Fresh Native Dev Start

From the repo root:

```bash
./dev-fresh
```

## Ports

Source dev uses backend port `3857`, so it can run while the installed app keeps production port `3847`.

## Agent Runtimes

RalphX needs at least one supported agent runtime installed and authenticated:

- [Claude CLI](https://docs.anthropic.com/en/docs/claude-code)
- [Codex CLI](https://developers.openai.com/codex/cli)

Harness controls are exposed in the desktop app:

- `Settings -> General -> Execution Agents` for worker, reviewer, re-executor, and merger lanes
- `Settings -> Ideation -> Ideation Agents` for ideation, verifier, and specialist lanes
