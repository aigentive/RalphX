#!/usr/bin/env node

/**
 * P-11 (client half) + P-18 (wrapper half) — remote transport drift scan.
 *
 * The transport seams only hold if EVERY production call goes through them. This
 * scan parses `frontend/src` with the TypeScript AST and fails on the five ways a
 * call site can slip past:
 *
 *   1. A dynamic `invoke` command expression. A command name that is not a literal
 *      cannot be classified as remote-registered or local-only, so it defeats the
 *      whole inventory. Forwarders (`typedInvoke(cmd, ...)` helpers that pass their
 *      OWN parameter through) are resolved rather than flagged — the literal lives
 *      at their call sites, which the scan follows. Invoke roots are matched through
 *      import ALIASES (`invoke as primitiveInvoke`) and module-scope string constants
 *      are folded, so neither spelling hides a call site from the inventory.
 *   2. Any reference to `backendApiUrl`/`backendBaseUrl`/`backendApiPath` outside
 *      `api/backend.ts` — a site building a local backend URL instead of going
 *      through `backendFetch`, which would keep hitting this Mac's backend while a
 *      remote environment is active. References, not `fetch()` argument text: the URL
 *      and the `fetch` are often one line apart.
 *   3. A cross-origin escape inside the transport wrapper itself (`fetch`,
 *      `window.fetch`, `WebSocket`, `EventSource`, `XMLHttpRequest`, including
 *      `globalThis.`-qualified forms). Remote traffic is Rust-proxied precisely so the
 *      webview never holds the bearer or opens a socket (C-15).
 *   4. `#tauri-core-primitive` — the un-aliased Tauri core — imported outside
 *      `src/lib/remote`, which would route a caller past the wrapper entirely.
 *   5. A deep `@tauri-apps/api/core.js` / `@tauri-apps/api/core/…` import anywhere:
 *      the Vite alias matches the bare specifier only, so a deep path silently strands
 *      that caller on local IPC while a remote environment is active.
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

/** Local URL construction. A reference to ANY of these outside the owner is a bypass in progress. */
const BACKEND_URL_HELPERS = new Set([
  "backendApiUrl",
  "backendBaseUrl",
  "backendApiPath",
]);

/**
 * The Vite alias redirects the specifier `@tauri-apps/api/core` EXACTLY. A deep path resolves to
 * the real module at runtime, stranding that caller on local IPC while a remote environment is
 * active — the same escape `#tauri-core-primitive` is fenced for, spelled differently.
 */
const UNALIASED_CORE_SPECIFIER = /^@tauri-apps\/api\/core[./]/;

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

/**
 * Local names an invoke root was imported UNDER, with its command-argument index.
 *
 * `import { invoke as primitiveInvoke } from "…"` is the style the transport wrapper itself uses,
 * so matching roots by bare name alone would leave every aliased call site both un-inventoried and
 * un-flagged — invisible to the scan rather than caught by it.
 */
function invokeRootAliases(sourceFile) {
  const aliases = new Map();
  for (const statement of sourceFile.statements) {
    if (!ts.isImportDeclaration(statement)) continue;
    const bindings = statement.importClause?.namedBindings;
    if (!bindings || !ts.isNamedImports(bindings)) continue;
    for (const element of bindings.elements) {
      const exported = (element.propertyName ?? element.name).text;
      if (INVOKE_ROOTS.has(exported)) {
        aliases.set(element.name.text, INVOKE_ROOTS.get(exported));
      }
    }
  }
  return aliases;
}

/**
 * Module-scope `const NAME = "literal"` bindings.
 *
 * A command named by a module constant (`invoke(REMOTE_INVOKE_COMMAND, …)`) is a literal the
 * inventory can classify, not a dynamic expression — folding it is what keeps the P-11 rule
 * ("every production command name must be a literal") from punishing a named constant.
 */
function stringConstants(sourceFile) {
  const constants = new Map();
  for (const statement of sourceFile.statements) {
    if (!ts.isVariableStatement(statement)) continue;
    for (const declaration of statement.declarationList.declarations) {
      if (!ts.isIdentifier(declaration.name)) continue;
      const literal = literalCommand(declaration.initializer);
      if (literal !== null) constants.set(declaration.name.text, literal);
    }
  }
  return constants;
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
    const forwarders = new Map([
      ...INVOKE_ROOTS,
      ...invokeRootAliases(sourceFile),
    ]);
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
    const local =
      forwarders.perFile.get(sourceFile.fileName) ??
      new Map([...INVOKE_ROOTS, ...invokeRootAliases(sourceFile)]);
    const imported = importedNames(sourceFile);
    const constants = stringConstants(sourceFile);
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
      const command =
        literalCommand(commandArg) ??
        (commandArg !== undefined && ts.isIdentifier(commandArg)
          ? (constants.get(commandArg.text) ?? null)
          : null);
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

/**
 * Any reference to the local URL helpers outside their owning module.
 *
 * Deliberately NOT "a `fetch()` whose argument text mentions them": `const url =
 * backendApiUrl(x); fetch(url)` is the same bypass one line apart, and an aliased import
 * (`backendApiUrl as u`) hides the name from every use site — so the import binding is flagged
 * too. Nothing outside `api/backend.ts` has a legitimate reason to build a local backend URL;
 * `backendFetch` is the seam.
 */
export function collectFetchBypasses(parsedFiles) {
  const violations = [];
  for (const sourceFile of parsedFiles) {
    if (sourceFile.fileName === BACKEND_URL_OWNER) continue;
    walk(sourceFile, (node) => {
      if (!ts.isIdentifier(node) || !BACKEND_URL_HELPERS.has(node.text)) return;
      const parent = node.parent;
      // `x.backendApiUrl` names a member of something else, not this helper.
      if (parent && ts.isPropertyAccessExpression(parent) && parent.name === node) return;
      violations.push({
        file: sourceFile.fileName,
        line: lineOf(sourceFile, node),
        detail: `${node.text} outside api/backend.ts — build the request with backendFetch()`,
      });
    });
  }
  return violations;
}

/** `fetch(...)`, `window.fetch(...)`, `globalThis.fetch(...)`, `self.fetch(...)`. */
function isFetchCall(node) {
  if (!ts.isCallExpression(node)) return false;
  const expression = node.expression;
  if (ts.isIdentifier(expression)) return expression.text === "fetch";
  return (
    ts.isPropertyAccessExpression(expression) &&
    ts.isIdentifier(expression.name) &&
    expression.name.text === "fetch"
  );
}

/** The constructed global's name, through `new X()` or `new window.X()`. */
function constructedGlobalName(node) {
  if (!ts.isNewExpression(node)) return null;
  const expression = node.expression;
  if (ts.isIdentifier(expression)) return expression.text;
  if (ts.isPropertyAccessExpression(expression) && ts.isIdentifier(expression.name)) {
    return expression.name.text;
  }
  return null;
}

/**
 * P-18: the transport wrapper must not open a connection from the webview.
 *
 * Property-access callees count: `window.fetch(…)` / `globalThis.WebSocket` reach the same
 * runtime as the bare names and would otherwise walk straight past a bare-identifier check.
 */
export function collectWrapperNetworkEscapes(parsedFiles) {
  const violations = [];
  for (const sourceFile of parsedFiles) {
    if (!sourceFile.fileName.startsWith(TRANSPORT_DIR)) continue;
    walk(sourceFile, (node) => {
      if (isFetchCall(node)) {
        violations.push({
          file: sourceFile.fileName,
          line: lineOf(sourceFile, node),
          detail: "fetch() inside the transport wrapper — remote traffic is Rust-proxied",
        });
      }
      const constructed = constructedGlobalName(node);
      if (constructed !== null && NETWORK_ESCAPE_GLOBALS.has(constructed)) {
        violations.push({
          file: sourceFile.fileName,
          line: lineOf(sourceFile, node),
          detail: `new ${constructed}() inside the transport wrapper — remote traffic is Rust-proxied`,
        });
      }
    });
  }
  return violations;
}

/**
 * A deep import of the Tauri core bypasses the Vite alias, which matches the bare specifier only.
 * Repo-wide: the wrapper has `#tauri-core-primitive` for its own un-aliased access and nothing
 * else may reach the real module under any spelling.
 */
export function collectUnaliasedCoreImports(parsedFiles) {
  const violations = [];
  for (const sourceFile of parsedFiles) {
    walk(sourceFile, (node) => {
      if (!ts.isStringLiteral(node) && !ts.isNoSubstitutionTemplateLiteral(node)) return;
      if (!UNALIASED_CORE_SPECIFIER.test(node.text)) return;
      violations.push({
        file: sourceFile.fileName,
        line: lineOf(sourceFile, node),
        detail: `${node.text} bypasses the @tauri-apps/api/core alias — import the bare specifier`,
      });
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

  const aliasFixture = parse(
    "frontend/src/api/aliased.ts",
    `
    import { invoke as inv } from "@tauri-apps/api/core";
    const NAMED_COMMAND = "list_widgets";
    export const api = {
      one: () => inv("get_widget", {}),
      two: () => inv(NAMED_COMMAND, {}),
      three: () => inv(pickCommand(), {}),
    };
    `
  );
  const aliasForwarders = collectForwarders([aliasFixture]);
  const aliasSites = collectInvokeCallSites([aliasFixture], aliasForwarders);
  const aliasCommands = aliasSites.map((site) => site.command);
  check("inventories an aliased invoke import", aliasCommands.includes("get_widget"));
  check(
    "folds a module-scope command constant into a literal",
    aliasCommands.includes("list_widgets")
  );
  check(
    "still flags a dynamic command behind an aliased import",
    aliasCommands.filter((command) => command === null).length === 1
  );

  const bypass = collectFetchBypasses([
    parse(
      "frontend/src/api/thing.ts",
      `const r = await fetch(backendApiUrl("x"), { method: "POST" });`
    ),
  ]);
  check("flags a fetch(backendApiUrl(...)) bypass", bypass.length === 1);
  check(
    "flags a backend URL built one line before the fetch",
    collectFetchBypasses([
      parse(
        "frontend/src/api/indirect.ts",
        `const url = backendApiUrl("x");\nconst r = await fetch(url);`
      ),
    ]).length === 1
  );
  check(
    "flags an aliased backend URL helper import",
    collectFetchBypasses([
      parse(
        "frontend/src/api/aliasedUrl.ts",
        `import { backendApiUrl as u } from "@/api/backend";\nawait fetch(u("x"));`
      ),
    ]).length === 1
  );
  check(
    "ignores an unrelated member named backendApiUrl",
    collectFetchBypasses([
      parse("frontend/src/api/member.ts", `const v = config.backendApiUrl;`),
    ]).length === 0
  );
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
  check(
    "flags window/globalThis network escapes inside the wrapper",
    collectWrapperNetworkEscapes([
      parse(
        `${TRANSPORT_DIR}sneaky.ts`,
        `await window.fetch("https://host/x");\nconst s = new globalThis.WebSocket("wss://host");`
      ),
    ]).length === 2
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
    "flags a deep import that bypasses the core alias",
    collectUnaliasedCoreImports([
      parse(
        "frontend/src/api/deep.ts",
        `import { invoke } from "@tauri-apps/api/core.js";`
      ),
    ]).length === 1
  );
  check(
    "allows the aliased bare core specifier",
    collectUnaliasedCoreImports([
      parse("frontend/src/api/fine.ts", `import { invoke } from "@tauri-apps/api/core";`),
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
  console.log("PASS: drift-scan self-test (26 detector cases)");
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
  ...collectUnaliasedCoreImports(parsedFiles),
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
