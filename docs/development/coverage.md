# Code Coverage

The README shows aggregate coverage for `main`. Suite-level coverage uses Codecov flags so each report can be tracked independently while still contributing to the project total.

## Suites

| Suite | Coverage | Covered Areas | CI Command | Notes |
|---|---|---|---|---|
| Total | [![Total coverage](https://img.shields.io/codecov/c/github/aigentive/ralphx.app/main?token=DXF9O681JQ&label=total&logo=codecov&logoColor=white)](https://app.codecov.io/gh/aigentive/ralphx.app) | All uploaded coverage suites | Final `Publish Codecov Coverage` job | Aggregate project coverage shown in the README badge. |
| Rust library | [![Rust lib coverage](https://img.shields.io/codecov/c/github/aigentive/ralphx.app/main?token=DXF9O681JQ&flag=rust-lib&label=rust-lib&logo=codecov&logoColor=white)](https://app.codecov.io/gh/aigentive/ralphx.app/tree/main?flags%5B0%5D=rust-lib) | [src-tauri/](../../src-tauri/) | `cargo llvm-cov nextest --profile ci --lib` | Backend library coverage. |
| Rust IPC contracts | [![Rust IPC coverage](https://img.shields.io/codecov/c/github/aigentive/ralphx.app/main?token=DXF9O681JQ&flag=rust-ipc&label=rust-ipc&logo=codecov&logoColor=white)](https://app.codecov.io/gh/aigentive/ralphx.app/tree/main?flags%5B0%5D=rust-ipc) | [src-tauri/tests/](../../src-tauri/tests/) and IPC command paths in [src-tauri/](../../src-tauri/) | `cargo llvm-cov nextest --profile ci --test ... ipc_contract` | Focused integration coverage for Tauri command contracts. |
| Frontend | [![Frontend coverage](https://img.shields.io/codecov/c/github/aigentive/ralphx.app/main?token=DXF9O681JQ&flag=frontend&label=frontend&logo=codecov&logoColor=white)](https://app.codecov.io/gh/aigentive/ralphx.app/tree/main?flags%5B0%5D=frontend) | [frontend/src/](../../frontend/src/) and [frontend/tests/](../../frontend/tests/) | `npm run test:coverage` in [frontend/](../../frontend/) | React and TypeScript coverage from Vitest's V8 coverage provider. |
| Internal MCP | [![Internal MCP coverage](https://img.shields.io/codecov/c/github/aigentive/ralphx.app/main?token=DXF9O681JQ&flag=plugin-internal-mcp&label=internal-mcp&logo=codecov&logoColor=white)](https://app.codecov.io/gh/aigentive/ralphx.app/tree/main?flags%5B0%5D=plugin-internal-mcp) | [plugins/app/ralphx-mcp-server/](../../plugins/app/ralphx-mcp-server/) | `npm run test:coverage` in the package | Internal agent MCP server coverage from its Vitest suite. |
| External MCP | [![External MCP coverage](https://img.shields.io/codecov/c/github/aigentive/ralphx.app/main?token=DXF9O681JQ&flag=plugin-external-mcp&label=external-mcp&logo=codecov&logoColor=white)](https://app.codecov.io/gh/aigentive/ralphx.app/tree/main?flags%5B0%5D=plugin-external-mcp) | [plugins/app/ralphx-external-mcp/](../../plugins/app/ralphx-external-mcp/) | `npm run test:coverage` in the package | External HTTP MCP bridge coverage from its Vitest suite. |

## CI Behavior

The `Coverage Reports` workflow runs on pull requests to `main`, pushes to `main`, and manual dispatch.

PRs run only affected coverage suites when possible. Pushes to `main` run every suite and refresh the default-branch Codecov badges.

Coverage jobs publish raw artifacts first. The final `Publish Codecov Coverage` job downloads those artifacts and uploads the selected reports to Codecov together at the end of the workflow. That keeps the public badges from reflecting long-running partial uploads while Rust and frontend reports are still being generated.

Each suite produces:

- an LCOV report for the final Codecov publish job
- a Markdown summary in the GitHub job summary
- raw coverage artifacts for short-term inspection

## Codecov Flags

| Flag | Meaning |
|---|---|
| `rust-lib` | Rust backend library tests |
| `rust-ipc` | Rust IPC command contract tests |
| `frontend` | Frontend Vitest coverage |
| `plugin-internal-mcp` | Internal MCP server package coverage |
| `plugin-external-mcp` | External MCP bridge package coverage |

Carryforward is enabled in [codecov.yml](../../codecov.yml), so path-scoped PR runs can keep untouched suite baselines until the next full `main` upload.
