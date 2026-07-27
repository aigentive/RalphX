#!/usr/bin/env node

/**
 * P-11 (client half) + P-18 (wrapper half) — remote transport drift scan.
 *
 * The transport seams only hold if EVERY production call goes through them. This
 * scan parses `frontend/src` with the TypeScript AST and fails on the four ways a
 * call site can slip past:
 *
 *   1. A dynamic `invoke` command expression. A command name that is not a literal
 *      cannot be classified as remote-registered or local-only, so it defeats the
 *      whole inventory. Forwarders (`typedInvoke(cmd, ...)` helpers that pass their
 *      OWN parameter through) are resolved rather than flagged — the literal lives
 *      at their call sites, which the scan follows.
 *   2. A `fetch()` built from `backendApiUrl`/`backendBaseUrl` outside
 *      `api/backend.ts` — i.e. a site that bypasses `backendFetch` and would keep
 *      hitting this Mac's backend while a remote environment is active.
 *   3. A cross-origin escape inside the transport wrapper itself (`fetch`,
 *      `WebSocket`, `EventSource`, `XMLHttpRequest`). Remote traffic is Rust-proxied
 *      precisely so the webview never holds the bearer or opens a socket (C-15).
 *   4. `#tauri-core-primitive` — the un-aliased Tauri core — imported outside
 *      `src/lib/remote`, which would route a caller past the wrapper entirely.
 *
 * Plus a RATCHET on the command inventory: every literal command name is either
 * remote-registered (host facade) or listed in `local-only-commands.ts`. Anything
 * else is "unclassified" and must appear in the checked-in baseline. New
 * unclassified names fail; names that became classified must be pruned (run with
 * --update-baseline). The count reaches zero in PR 3.1, after which the baseline
 * file goes away.
 *
 * Usage:
 *   node scripts/check-remote-transport-drift.mjs [repoRoot]
 *   node scripts/check-remote-transport-drift.mjs --self-test
 *   node scripts/check-remote-transport-drift.mjs --update-baseline
 */

import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

const args = process.argv.slice(2);
const selfTest = args.includes("--self-test");
const updateBaseline = args.includes("--update-baseline");
const positional = args.find((arg) => !arg.startsWith("--"));
const repoRoot = path.resolve(positional ?? process.cwd());

const frontendRoot = path.join(repoRoot, "frontend");
const sourceRoot = path.join(frontendRoot, "src");
const registryPath = path.join(
  repoRoot,
  "src-tauri",
  "src",
  "remote_server",
  "registry.rs"
);
const localOnlyPath = path.join(sourceRoot, "lib", "remote", "local-only-commands.ts");
const baselinePath = path.join(repoRoot, "scripts", "remote-transport-drift-baseline.json");

const require = createRequire(path.join(frontendRoot, "package.json"));
const ts = require("typescript");

// ---------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------

/** `api/backend.ts` OWNS the local URL construction; it is the seam, not a bypass. */
const BACKEND_URL_OWNER = "frontend/src/api/backend.ts";
/** The transport modules may reach the un-aliased core; nothing else may. */
const TRANSPORT_DIR = "frontend/src/lib/remote/";

const EXCLUDED_DIRECTORIES = new Set([
  "frontend/src/mocks",
  "frontend/src/api-mock",
  "frontend/src/test",
]);
const TEST_FILE_PATTERN = /\.(?:test|spec)\.[cm]?[jt]sx?$/;
const SOURCE_FILE_PATTERN = /\.[cm]?[jt]sx?$/;

/** Roots of the invoke forwarder graph. Everything else is discovered. */
const INVOKE_ROOTS = new Map([
  ["invoke", 0],
  ["typedInvoke", 0],
  ["typedInvokeWithTransform", 0],
]);

const NETWORK_ESCAPE_GLOBALS = new Set([
  "WebSocket",
  "EventSource",
  "XMLHttpRequest",
]);

function toRepoPath(filePath) {
  return path.relative(repoRoot, filePath).split(path.sep).join("/");
}

function isExcluded(filePath) {
  const repoPath = toRepoPath(filePath);
  return (
    TEST_FILE_PATTERN.test(repoPath) ||
    [...EXCLUDED_DIRECTORIES].some(
      (directory) => repoPath === directory || repoPath.startsWith(`${directory}/`)
    )
  );
}

function sourceFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      return isExcluded(entryPath) ? [] : sourceFiles(entryPath);
    }
    return entry.isFile() &&
      SOURCE_FILE_PATTERN.test(entry.name) &&
      !isExcluded(entryPath)
      ? [entryPath]
      : [];
  });
}

// ---------------------------------------------------------------------------
// AST helpers
// ---------------------------------------------------------------------------

export function parse(repoPath, source) {
  return ts.createSourceFile(
    repoPath,
    source,
    ts.ScriptTarget.Latest,
    /* setParentNodes */ true,
    repoPath.endsWith("x") ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  );
}

function lineOf(sourceFile, node) {
  return sourceFile.getLineAndCharacterOfPosition(node.getStart(sourceFile)).line + 1;
}

function walk(node, visit) {
  visit(node);
  ts.forEachChild(node, (child) => walk(child, visit));
}

/** `invoke(...)` → "invoke". Bare identifier callees only. */
function calleeName(node) {
  if (!ts.isCallExpression(node)) return null;
  return ts.isIdentifier(node.expression) ? node.expression.text : null;
}

/**
 * `core.invoke(...)` → "invoke", but ONLY for the invoke roots.
 *
 * Discovered forwarders are matched by bare identifier alone: their names are
 * ordinary words (`catalog`, `get`) that collide with unrelated methods across the
 * tree, and treating `someKeys.catalog(id)` as an invoke would manufacture false
 * dynamic-command failures.
 */
function rootMemberCalleeName(node) {
  if (!ts.isCallExpression(node)) return null;
  const expression = node.expression;
  if (ts.isPropertyAccessExpression(expression) && ts.isIdentifier(expression.name)) {
    return INVOKE_ROOTS.has(expression.name.text) ? expression.name.text : null;
  }
  return null;
}

/** Named import bindings, so an exported forwarder resolves in its consumers. */
function importedNames(sourceFile) {
  const names = new Set();
  for (const statement of sourceFile.statements) {
    if (!ts.isImportDeclaration(statement)) continue;
    const bindings = statement.importClause?.namedBindings;
    if (bindings && ts.isNamedImports(bindings)) {
      for (const element of bindings.elements) names.add(element.name.text);
    }
    if (statement.importClause?.name) names.add(statement.importClause.name.text);
  }
  return names;
}

function isExported(fn) {
  const declaration =
    ts.isVariableDeclaration(fn.parent) && fn.parent.parent?.parent
      ? fn.parent.parent.parent
      : fn;
  const modifiers = ts.canHaveModifiers(declaration)
    ? ts.getModifiers(declaration)
    : undefined;
  return (modifiers ?? []).some(
    (modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword
  );
}

function literalCommand(argument) {
  if (argument === undefined) return null;
  if (ts.isStringLiteral(argument) || ts.isNoSubstitutionTemplateLiteral(argument)) {
    return argument.text;
  }
  return null;
}

/** The nearest enclosing function-like node, and the name it is bound to. */
function enclosingFunction(node) {
  let current = node.parent;
  while (current) {
    if (
      ts.isFunctionDeclaration(current) ||
      ts.isFunctionExpression(current) ||
      ts.isArrowFunction(current) ||
      ts.isMethodDeclaration(current)
    ) {
      return current;
    }
    current = current.parent;
  }
  return null;
}

function functionName(fn) {
  if ((ts.isFunctionDeclaration(fn) || ts.isMethodDeclaration(fn)) && fn.name) {
    return ts.isIdentifier(fn.name) ? fn.name.text : null;
  }
  const parent = fn.parent;
  if (!parent) return null;
  if (ts.isVariableDeclaration(parent) && ts.isIdentifier(parent.name)) {
    return parent.name.text;
  }
  if (ts.isPropertyAssignment(parent) && ts.isIdentifier(parent.name)) {
    return parent.name.text;
  }
  return null;
}

/** Index of `fn`'s parameter named `identifier`, or -1. */
function parameterIndex(fn, identifier) {
  return fn.parameters.findIndex(
    (parameter) =>
      ts.isIdentifier(parameter.name) && parameter.name.text === identifier
  );
}

// ---------------------------------------------------------------------------
// Detectors (pure — exercised by --self-test)
// ---------------------------------------------------------------------------

/**
 * Discovers invoke forwarders per file, to a fixpoint: a named function that passes
 * one of its OWN parameters as the command argument of an invoke-like call is itself
 * invoke-like (the ~19 local `typedInvoke` helpers, `catalog(command, input)`, …).
 *
 * File-scoped by design. Forwarder names are ordinary words that repeat across the
 * tree, so a global name map produces cross-file false positives; an EXPORTED
 * forwarder is still resolved in files that import it by name.
 *
 * @returns `{ perFile: Map<fileName, Map<name, index>>, exported: Map<name, index> }`
 */
export function collectForwarders(parsedFiles) {
  const perFile = new Map();
  const exported = new Map();

  for (const sourceFile of parsedFiles) {
    const forwarders = new Map(INVOKE_ROOTS);
    let changed = true;
    while (changed) {
      changed = false;
      walk(sourceFile, (node) => {
        const callee = calleeName(node) ?? rootMemberCalleeName(node);
        if (callee === null || !forwarders.has(callee)) return;
        const commandArg = node.arguments[forwarders.get(callee)];
        if (commandArg === undefined || !ts.isIdentifier(commandArg)) return;
        const fn = enclosingFunction(node);
        if (fn === null) return;
        const index = parameterIndex(fn, commandArg.text);
        if (index === -1) return;
        const name = functionName(fn);
        if (name === null || forwarders.has(name)) return;
        forwarders.set(name, index);
        if (isExported(fn)) exported.set(name, index);
        changed = true;
      });
    }
    perFile.set(sourceFile.fileName, forwarders);
  }
  return { perFile, exported };
}

/**
 * Every invoke-like call site: `{ file, line, command }`, with `command === null`
 * for a dynamic expression that is NOT a forwarder's own parameter.
 */
export function collectInvokeCallSites(parsedFiles, forwarders) {
  const sites = [];
  for (const sourceFile of parsedFiles) {
    const local = forwarders.perFile.get(sourceFile.fileName) ?? new Map(INVOKE_ROOTS);
    const imported = importedNames(sourceFile);
    const resolve = (name) => {
      if (local.has(name)) return local.get(name);
      if (imported.has(name) && forwarders.exported.has(name)) {
        return forwarders.exported.get(name);
      }
      return undefined;
    };

    walk(sourceFile, (node) => {
      const callee = calleeName(node) ?? rootMemberCalleeName(node);
      if (callee === null) return;
      const commandIndex = resolve(callee);
      if (commandIndex === undefined) return;

      const commandArg = node.arguments[commandIndex];
      const command = literalCommand(commandArg);
      if (command !== null) {
        sites.push({ file: sourceFile.fileName, line: lineOf(sourceFile, node), command });
        return;
      }
      // A forwarder passing its own parameter through is resolved indirection, not
      // a dynamic name — the literal lives at ITS call sites.
      if (commandArg !== undefined && ts.isIdentifier(commandArg)) {
        const fn = enclosingFunction(node);
        if (fn !== null && parameterIndex(fn, commandArg.text) !== -1) return;
      }
      sites.push({
        file: sourceFile.fileName,
        line: lineOf(sourceFile, node),
        command: null,
      });
    });
  }
  return sites;
}

/** `fetch(...)` built from the backend URL helpers outside their owning module. */
export function collectFetchBypasses(parsedFiles) {
  const violations = [];
  for (const sourceFile of parsedFiles) {
    if (sourceFile.fileName === BACKEND_URL_OWNER) continue;
    walk(sourceFile, (node) => {
      if (calleeName(node) !== "fetch") return;
      const uses = node.arguments.some((argument) =>
        /\bbackend(ApiUrl|BaseUrl|ApiPath)\b/.test(argument.getText(sourceFile))
      );
      if (uses) {
        violations.push({
          file: sourceFile.fileName,
          line: lineOf(sourceFile, node),
          detail: "fetch() built from backendApiUrl/backendBaseUrl — use backendFetch()",
        });
      }
    });
  }
  return violations;
}

/** P-18: the transport wrapper must not open a connection from the webview. */
export function collectWrapperNetworkEscapes(parsedFiles) {
  const violations = [];
  for (const sourceFile of parsedFiles) {
    if (!sourceFile.fileName.startsWith(TRANSPORT_DIR)) continue;
    walk(sourceFile, (node) => {
      if (ts.isCallExpression(node) && calleeName(node) === "fetch") {
        violations.push({
          file: sourceFile.fileName,
          line: lineOf(sourceFile, node),
          detail: "fetch() inside the transport wrapper — remote traffic is Rust-proxied",
        });
      }
      if (
        ts.isNewExpression(node) &&
        ts.isIdentifier(node.expression) &&
        NETWORK_ESCAPE_GLOBALS.has(node.expression.text)
      ) {
        violations.push({
          file: sourceFile.fileName,
          line: lineOf(sourceFile, node),
          detail: `new ${node.expression.text}() inside the transport wrapper — remote traffic is Rust-proxied`,
        });
      }
    });
  }
  return violations;
}

/** The un-aliased core is the wrapper's private door; nobody else may use it. */
export function collectPrimitiveSpecifierEscapes(parsedFiles) {
  const violations = [];
  for (const sourceFile of parsedFiles) {
    if (sourceFile.fileName.startsWith(TRANSPORT_DIR)) continue;
    walk(sourceFile, (node) => {
      if (
        (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) &&
        node.text === "#tauri-core-primitive"
      ) {
        violations.push({
          file: sourceFile.fileName,
          line: lineOf(sourceFile, node),
          detail:
            "#tauri-core-primitive outside src/lib/remote — import @tauri-apps/api/core instead",
        });
      }
    });
  }
  return violations;
}

// ---------------------------------------------------------------------------
// Classification inputs
// ---------------------------------------------------------------------------

/**
 * Command names registered on the host facade. Parsed rather than generated because
 * PR 1.3 owns `registry.rs` on a parallel lane; an absent or restructured block
 * degrades to "nothing registered" with a warning, never a false failure.
 */
export function parseRegisteredCommands(registrySource) {
  const start = registrySource.indexOf("crate::remote_commands! {");
  if (start === -1) return null;
  const block = registrySource.slice(start);
  return new Set([...block.matchAll(/^\s*"([a-z0-9_]+)"\s*=>/gm)].map((m) => m[1]));
}

export function parseLocalOnlyCommands(localOnlySource) {
  return new Set(
    [...localOnlySource.matchAll(/command:\s*"([a-z0-9_]+)"/g)].map((m) => m[1])
  );
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

function runSelfTest() {
  const failures = [];
  const check = (name, condition) => {
    if (!condition) failures.push(name);
  };

  const forwarderFixture = parse(
    "frontend/src/api/fixture.ts",
    `
    import { invoke } from "@tauri-apps/api/core";
    async function typedInvoke(cmd, args, schema) {
      const result = await invoke(cmd, args);
      return schema.parse(result);
    }
    async function catalog(command, input) {
      return typedInvoke(command, { input }, Schema);
    }
    export const api = {
      list: () => typedInvoke("list_tasks", {}, S),
      scopes: () => catalog("get_mcp_scopes", {}),
      bad: () => {
        const cmd = flag ? "a" : "b";
        return invoke(cmd, {});
      },
    };
    `
  );
  const forwarders = collectForwarders([forwarderFixture]);
  const fixtureForwarders = forwarders.perFile.get("frontend/src/api/fixture.ts");
  check("discovers a local typedInvoke forwarder", fixtureForwarders.has("typedInvoke"));
  check("discovers a transitive forwarder", fixtureForwarders.get("catalog") === 0);
  check(
    "keeps a discovered forwarder out of other files",
    collectInvokeCallSites(
      [parse("frontend/src/hooks/other.ts", "const k = someKeys.catalog(projectId, provider);")],
      forwarders
    ).length === 0
  );

  const sites = collectInvokeCallSites([forwarderFixture], forwarders);
  const literals = sites.filter((site) => site.command !== null).map((s) => s.command);
  check("inventories a direct literal", literals.includes("list_tasks"));
  check("inventories a literal through a transitive forwarder", literals.includes("get_mcp_scopes"));
  check(
    "flags a dynamic command expression",
    sites.some((site) => site.command === null)
  );
  check(
    "does NOT flag a forwarder passing its own parameter",
    sites.filter((site) => site.command === null).length === 1
  );

  const bypass = collectFetchBypasses([
    parse(
      "frontend/src/api/thing.ts",
      `const r = await fetch(backendApiUrl("x"), { method: "POST" });`
    ),
  ]);
  check("flags a fetch(backendApiUrl(...)) bypass", bypass.length === 1);
  check(
    "exempts the backend.ts seam itself",
    collectFetchBypasses([
      parse(BACKEND_URL_OWNER, `const r = await fetch(backendApiUrl(e), init);`),
    ]).length === 0
  );
  check(
    "ignores a fetch to an unrelated origin",
    collectFetchBypasses([
      parse("frontend/src/api/release-notes.ts", `await fetch(releasePageUrl(p), {});`),
    ]).length === 0
  );

  const escapes = collectWrapperNetworkEscapes([
    parse(
      `${TRANSPORT_DIR}bad.ts`,
      `export async function go() { await fetch("https://host/x"); const s = new WebSocket("wss://host"); }`
    ),
  ]);
  check("flags fetch inside the transport wrapper", escapes.length === 2);
  check(
    "allows fetch outside the transport wrapper",
    collectWrapperNetworkEscapes([
      parse("frontend/src/api/other.ts", `await fetch("https://x");`),
    ]).length === 0
  );

  const primitive = collectPrimitiveSpecifierEscapes([
    parse("frontend/src/api/sneaky.ts", `import { invoke } from "#tauri-core-primitive";`),
  ]);
  check("flags the primitive specifier outside src/lib/remote", primitive.length === 1);
  check(
    "allows the primitive specifier inside src/lib/remote",
    collectPrimitiveSpecifierEscapes([
      parse(`${TRANSPORT_DIR}invoke.ts`, `import { invoke } from "#tauri-core-primitive";`),
    ]).length === 0
  );

  check(
    "parses registered command names",
    parseRegisteredCommands(
      `crate::remote_commands! {\n    "health_check" => crate::commands::health::health_check {\n`
    )?.has("health_check") === true
  );
  check(
    "degrades to null when the registry block is absent",
    parseRegisteredCommands("// placeholder") === null
  );
  check(
    "parses the local-only registry",
    parseLocalOnlyCommands(`{ command: "remote_invoke", disposition: "run-locally" }`).has(
      "remote_invoke"
    )
  );

  if (failures.length > 0) {
    console.error("FAIL: drift-scan self-test");
    failures.forEach((failure) => console.error(`  ${failure}`));
    process.exit(1);
  }
  console.log("PASS: drift-scan self-test (16 detector cases)");
  process.exit(0);
}

if (selfTest) {
  runSelfTest();
}

// ---------------------------------------------------------------------------
// Scan
// ---------------------------------------------------------------------------

if (!fs.existsSync(sourceRoot)) {
  console.error(`FAIL: missing frontend source directory: ${toRepoPath(sourceRoot)}`);
  process.exit(1);
}

const parsedFiles = sourceFiles(sourceRoot).map((filePath) =>
  parse(toRepoPath(filePath), fs.readFileSync(filePath, "utf8"))
);

const forwarders = collectForwarders(parsedFiles);
const callSites = collectInvokeCallSites(parsedFiles, forwarders);

const hardFailures = [
  ...callSites
    .filter((site) => site.command === null)
    .map((site) => ({
      file: site.file,
      line: site.line,
      detail:
        "dynamic invoke command expression — every production command name must be a literal (P-11)",
    })),
  ...collectFetchBypasses(parsedFiles),
  ...collectWrapperNetworkEscapes(parsedFiles),
  ...collectPrimitiveSpecifierEscapes(parsedFiles),
];

if (hardFailures.length > 0) {
  console.error("FAIL: remote transport drift");
  for (const failure of hardFailures) {
    console.error(`  ${failure.file}:${failure.line} — ${failure.detail}`);
  }
  process.exit(1);
}

// --- Command inventory ratchet ---------------------------------------------

const registered = fs.existsSync(registryPath)
  ? parseRegisteredCommands(fs.readFileSync(registryPath, "utf8"))
  : null;
if (registered === null) {
  console.warn(
    "WARN: no remote_commands! block found; treating the host facade as empty (PR 1.3 in flight)"
  );
}
const localOnly = fs.existsSync(localOnlyPath)
  ? parseLocalOnlyCommands(fs.readFileSync(localOnlyPath, "utf8"))
  : new Set();

const commands = [...new Set(callSites.map((site) => site.command))].sort();
const unclassified = commands.filter(
  (command) => !(registered ?? new Set()).has(command) && !localOnly.has(command)
);

const baseline = fs.existsSync(baselinePath)
  ? JSON.parse(fs.readFileSync(baselinePath, "utf8"))
  : { unclassifiedCommands: [] };
const baselineSet = new Set(baseline.unclassifiedCommands ?? []);

if (updateBaseline) {
  fs.writeFileSync(
    baselinePath,
    `${JSON.stringify(
      {
        note: "P-11 ratchet. Commands invoked by the frontend that are neither remote-registered nor local-only. This list may only shrink; PR 3.1 drives it to zero and deletes this file.",
        unclassifiedCommands: unclassified,
      },
      null,
      2
    )}\n`
  );
  console.log(
    `Updated baseline: ${unclassified.length} unclassified command(s) of ${commands.length}.`
  );
  process.exit(0);
}

const added = unclassified.filter((command) => !baselineSet.has(command));
const stale = [...baselineSet].filter((command) => !unclassified.includes(command)).sort();

if (added.length > 0 || stale.length > 0) {
  console.error("FAIL: remote transport command inventory drift");
  if (added.length > 0) {
    console.error(
      `  ${added.length} command(s) are neither remote-registered nor listed in local-only-commands.ts:`
    );
    added.forEach((command) => console.error(`    ${command}`));
    console.error(
      "  Register them on the host facade, add them to local-only-commands.ts with a reason,"
    );
    console.error("  or record them deliberately: node scripts/check-remote-transport-drift.mjs --update-baseline");
  }
  if (stale.length > 0) {
    console.error(
      `  ${stale.length} baseline entr(ies) are now classified or gone — the ratchet must tighten:`
    );
    stale.forEach((command) => console.error(`    ${command}`));
    console.error("  Prune them: node scripts/check-remote-transport-drift.mjs --update-baseline");
  }
  process.exit(1);
}

console.log(
  `PASS: remote transport drift — ${commands.length} invoke command name(s), 0 dynamic, ` +
    `0 seam bypasses; ${unclassified.length} unclassified (baseline, → 0 in PR 3.1).`
);
