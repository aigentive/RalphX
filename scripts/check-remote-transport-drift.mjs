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
 * Plus a RATCHET on the command inventory: every literal command name resolves through
 * one of FOUR sources — remote-registered (host facade), listed in
 * `local-only-commands.ts` with a reason, matched by that file's `plugin:` PREFIX RULE
 * (Phase 2: the seven Tauri plugin namespaces act on THIS device and route locally), or
 * carrying a host-denied/deferred
 * `v1Resolution` in `docs/generated/remote-commands.json` (P-11 batch B0: commands
 * the facade denies or defers are not client-local and must not claim to be).
 *
 * The inventory itself spans TWO source sets, because the transport does:
 *
 *   - `frontend/src`, walked above; and
 *   - the `@tauri-apps/plugin-*` packages `frontend/src` imports. The Vite alias
 *     redirects `@tauri-apps/api/core` for the WHOLE module graph, node_modules
 *     included, so those packages' own `invoke("plugin:…")` calls ride this transport
 *     too — 77 import sites the scan used to be structurally unable to see, which made
 *     the census's "0 unclassified" claim blind rather than true. Their command literals
 *     are collected with the same AST machinery and classified by the prefix rule.
 * Anything else is "unclassified". P-11 COMPLETED in PR 3.1-b batch 14: the count is now ZERO
 * and the gate is permanent. The scan fails if any unclassified name appears, AND if the
 * checked-in baseline is non-empty — a recorded entry is a suppression, and the phase doc's
 * exit criterion is zero unclassified names with zero suppressions. `--update-baseline` is
 * correspondingly refused while unclassified names exist, so the list cannot quietly regrow;
 * the only way forward is to resolve each name.
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
const manifestPath = path.join(repoRoot, "docs", "generated", "remote-commands.json");

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

/// The P-11 exit criterion as a pure predicate (PR 3.1-b batch 14).
///
/// Returns the violation messages, empty when P-11 holds. Pure so `--self-test` can prove the
/// gate FAILS on a regrown baseline — a gate whose failure path is never exercised is not a
/// gate, and this one's whole job is to fail on a state that no longer occurs in the repo.
export function p11ExitViolations(unclassified, baselineSet) {
  const violations = [];
  if (unclassified.length > 0) {
    violations.push(
      `FAIL: P-11 requires zero unclassified invoke command names; ${unclassified.length} found:\n` +
        unclassified.map((command) => `    ${command}`).join("\n")
    );
  }
  if (baselineSet.size > 0) {
    violations.push(
      `FAIL: the P-11 baseline must be EMPTY; ${baselineSet.size} entr(ies) are recorded:\n` +
        [...baselineSet].sort().map((command) => `    ${command}`).join("\n") +
        "\n  P-11 completed in PR 3.1-b batch 14. A recorded baseline entry is a suppression," +
        "\n  and the phase doc's exit criterion is zero unclassified names with zero suppressions."
    );
  }
  return violations;
}

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

/**
 * The `plugin:` PREFIX RULE, read out of `local-only-commands.ts` (Phase 2).
 *
 * The policy lives in the seam, not here: this parses `PLUGIN_COMMAND_PREFIX` and the
 * reviewed `HOST_TARGETED_PLUGIN_COMMANDS` exception list rather than re-declaring either,
 * so the scan can never certify a routing rule the app does not actually apply.
 *
 * Fail-closed: an absent or restructured prefix declaration returns `null`, which classifies
 * NOTHING — every `plugin:` name then lands in the unclassified list and CI goes red. A
 * silently-stopped classifier would put the census straight back into the blind state this
 * whole rule exists to end.
 *
 * @returns `{ prefix, exceptions: Set<string> }`, or `null` when the rule is not present
 */
export function parsePluginPrefixRule(localOnlySource) {
  const prefixMatch = localOnlySource.match(
    /export const PLUGIN_COMMAND_PREFIX\s*=\s*"([^"]+)"/
  );
  if (!prefixMatch) return null;

  const listMatch = localOnlySource.match(
    /export const HOST_TARGETED_PLUGIN_COMMANDS[^=]*=\s*\[([\s\S]*?)\]/
  );
  // The list must EXIST even when empty — an exception mechanism that can vanish is not a
  // mechanism. Its absence is the same fail-closed case as a missing prefix.
  if (!listMatch) return null;

  return {
    prefix: prefixMatch[1],
    exceptions: new Set([...listMatch[1].matchAll(/"([^"]+)"/g)].map((m) => m[1])),
  };
}

/**
 * Does the prefix rule classify this name as client-local?
 *
 * An EXCEPTED name answers false on purpose: it leaves local-only classification entirely
 * and must then earn a registration or a ledger disposition, exactly like any other host
 * command. That is what makes an exception cost something instead of being a quiet hole.
 */
export function pluginRuleClassifies(cmd, rule) {
  if (rule === null) return false;
  return cmd.startsWith(rule.prefix) && !rule.exceptions.has(cmd);
}

/**
 * `@tauri-apps/plugin-*` packages imported anywhere in `frontend/src`.
 *
 * Static imports, `export … from`, and dynamic `import("…")` all count — `lib/open-external.ts`
 * reaches the opener plugin through a dynamic import, and missing it would leave the single
 * highest-traffic plugin family (29 sites) out of the census.
 */
export function collectPluginPackageSpecifiers(parsedFiles) {
  const specifiers = new Set();
  for (const sourceFile of parsedFiles) {
    walk(sourceFile, (node) => {
      if (!ts.isStringLiteral(node) && !ts.isNoSubstitutionTemplateLiteral(node)) return;
      if (!PLUGIN_PACKAGE_SPECIFIER.test(node.text)) return;
      const parent = node.parent;
      const isModuleSpecifier =
        (ts.isImportDeclaration(parent) && parent.moduleSpecifier === node) ||
        (ts.isExportDeclaration(parent) && parent.moduleSpecifier === node) ||
        (ts.isCallExpression(parent) &&
          parent.expression.kind === ts.SyntaxKind.ImportKeyword);
      if (isModuleSpecifier) specifiers.add(node.text);
    });
  }
  return specifiers;
}

/** `@tauri-apps/plugin-opener`, but not `@tauri-apps/plugin-opener/something`. */
const PLUGIN_PACKAGE_SPECIFIER = /^@tauri-apps\/plugin-[a-z0-9-]+$/;

/**
 * The ESM bundle a plugin package ships — the file the Vite alias rewrites `invoke` inside.
 *
 * `dist-js/index.js` only: `index.cjs` is the same source in another module format and parsing
 * both would double-report a dynamic command expression as two findings for one defect.
 *
 * Returns `null` when the package is not installed, which the caller treats as fail-closed
 * (blind is the state Phase 2 exists to end, so an uncomputable census is an error, not a pass).
 */
export function pluginPackageEntryFile(frontendRoot, specifier) {
  const entry = path.join(
    frontendRoot,
    "node_modules",
    ...specifier.split("/"),
    "dist-js",
    "index.js"
  );
  return fs.existsSync(entry) ? entry : null;
}

/**
 * The third classification source (P-11 batch B0).
 *
 * A large block of the gap is neither remote-registered nor client-local: they are host
 * commands the facade DENIES (`RiskClass::Denied`, or `SpawnsProcess`, which `class_permits`
 * admits only under `Elevated`) or DEFERS (`Elevated`, a v1 non-goal). Phase-doc key point 6
 * fixes their resolution as the ledger rows the manifest renders — explicitly NOT a
 * client-local reason, because they are not client-local and pretending otherwise would put a
 * false statement in `local-only-commands.ts`.
 *
 * The verdict is READ, never re-derived: `ralphx_remote_protocol::v1_resolution` owns it and
 * `capability_ledger_tests` renders it per row. Re-implementing `class_permits` here would fork
 * the authority, which is exactly the failure this indirection exists to avoid.
 *
 * Fail-closed in both directions:
 * - an absent, shapeless, or resolution-less row classifies NOTHING (a missing field can never
 *   read as "classified");
 * - an unknown resolution literal or a registered-and-refused row THROWS. A rename upstream must
 *   fail the scan loudly rather than silently stop classifying, and a row the registry serves
 *   while the ledger denies it is a contradiction the ratchet must not paper over.
 *
 * @param {unknown} manifest parsed `remote-commands.json`, or `null` when absent
 * @returns {Map<string, string>} command name → non-registerable resolution
 */
export function parseManifestClassifications(manifest) {
  const classified = new Map();
  const ledger = manifest && typeof manifest === "object" ? manifest.ledger : null;
  if (!Array.isArray(ledger)) return classified;

  for (const row of ledger) {
    if (!row || typeof row !== "object") continue;
    const { command, v1Resolution: resolution, registered } = row;
    if (typeof command !== "string" || resolution === undefined) continue;
    if (!MANIFEST_RESOLUTIONS.has(resolution)) {
      throw new Error(
        `unknown v1Resolution \`${resolution}\` on ledger row \`${command}\` — the manifest ` +
          "contract changed; update MANIFEST_RESOLUTIONS and re-audit the ratchet"
      );
    }
    if (resolution === "registerable") continue;
    if (registered === true) {
      throw new Error(
        `ledger/registry contradiction: \`${command}\` renders \`${resolution}\` but is ` +
          "registered on the facade — the scan would classify a name the host actually serves"
      );
    }
    classified.set(command, resolution);
  }
  return classified;
}

/** The closed set `V1_RESOLUTIONS` renders. Pinned by `protocol_contract.rs`. */
const MANIFEST_RESOLUTIONS = new Set([
  "registerable",
  "host-denied",
  "host-denied-spawns-process",
  "v1-deferred",
  // PR 3.1-b batch 9: a per-command audit found a property no v1 scope can accommodate. Unlike
  // its siblings this one is not derived from the class/capability pair, so the ledger pairs it
  // with a rendered finding and `batch9_audit_refusals_are_tied_to_a_live_pin` requires the
  // mechanism to be asserted by a live pinned-refusal test.
  "v1-audit-refused",
]);

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

  // --- P-11 batch B0: the manifest as a third classification source ---------
  const manifestFixture = {
    ledger: [
      { command: "open_terminal", v1Resolution: "host-denied", registered: false },
      { command: "run_setup", v1Resolution: "host-denied-spawns-process", registered: false },
      { command: "read_credential", v1Resolution: "v1-deferred", registered: false },
      { command: "fail_open_getter", v1Resolution: "v1-audit-refused", registered: false },
      { command: "list_tasks", v1Resolution: "registerable", registered: true },
      { command: "archive_task", v1Resolution: "registerable", registered: false },
    ],
  };
  const manifestClassified = parseManifestClassifications(manifestFixture);
  check(
    "classifies a manifest host-denied name",
    manifestClassified.get("open_terminal") === "host-denied"
  );
  check(
    "classifies a manifest SpawnsProcess name",
    manifestClassified.get("run_setup") === "host-denied-spawns-process"
  );
  check(
    "classifies a manifest v1-deferred name",
    manifestClassified.get("read_credential") === "v1-deferred"
  );
  check(
    "classifies a manifest v1-audit-refused name",
    manifestClassified.get("fail_open_getter") === "v1-audit-refused"
  );
  check(
    "fails a registered-and-audit-refused contradiction",
    (() => {
      try {
        parseManifestClassifications({
          ledger: [{ command: "x", v1Resolution: "v1-audit-refused", registered: true }],
        });
        return false;
      } catch {
        return true;
      }
    })()
  );
  check(
    "leaves a registerable name unclassified — it still needs a registration or a reason",
    !manifestClassified.has("archive_task") && !manifestClassified.has("list_tasks")
  );
  check(
    "a name absent from every source stays unclassified",
    !manifestClassified.has("never_heard_of_it")
  );
  check(
    "rejects an unknown resolution literal rather than trusting it",
    (() => {
      try {
        parseManifestClassifications({
          ledger: [{ command: "x", v1Resolution: "probably-fine", registered: false }],
        });
        return false;
      } catch {
        return true;
      }
    })()
  );
  check(
    "fails a registered-and-manifest-denied contradiction",
    (() => {
      try {
        parseManifestClassifications({
          ledger: [{ command: "x", v1Resolution: "host-denied", registered: true }],
        });
        return false;
      } catch {
        return true;
      }
    })()
  );
  check(
    "degrades to nothing-classified when the manifest is absent or shapeless",
    parseManifestClassifications(null).size === 0 &&
      parseManifestClassifications({}).size === 0
  );
  check(
    "treats a row with no resolution as unclassified, never as classified",
    parseManifestClassifications({
      ledger: [{ command: "x", registered: false }],
    }).size === 0
  );

  // --- Phase 2: the Tauri plugin surface -----------------------------------
  const pluginRuleSource = `
    export const PLUGIN_COMMAND_PREFIX = "plugin:";
    export const HOST_TARGETED_PLUGIN_COMMANDS: readonly string[] = [];
    export const LOCAL_ONLY_COMMANDS = [
      { command: "remote_invoke", disposition: "run-locally", reason: "…" },
    ];
  `;
  const pluginRule = parsePluginPrefixRule(pluginRuleSource);
  check("parses the plugin: prefix rule out of local-only-commands.ts", pluginRule?.prefix === "plugin:");
  check("reads an empty exception list as empty, not as absent", pluginRule?.exceptions.size === 0);
  check(
    "classifies a plugin command by prefix",
    pluginRuleClassifies("plugin:opener|open_url", pluginRule) &&
      pluginRuleClassifies("plugin:updater|check", pluginRule)
  );
  check(
    "classifies a plugin the app has not installed yet — it is a PREFIX rule, not a list",
    pluginRuleClassifies("plugin:some-future-plugin|do_thing", pluginRule)
  );
  check(
    "does not classify an ordinary host command",
    !pluginRuleClassifies("list_tasks", pluginRule) &&
      !pluginRuleClassifies("install_plugin:opener", pluginRule)
  );
  check(
    "leaves a REVIEWED EXCEPTION unclassified, so it must earn a registration or a ledger row",
    !pluginRuleClassifies(
      "plugin:opener|open_url",
      parsePluginPrefixRule(`
        export const PLUGIN_COMMAND_PREFIX = "plugin:";
        export const HOST_TARGETED_PLUGIN_COMMANDS: readonly string[] = ["plugin:opener|open_url"];
      `)
    )
  );
  check(
    "fails closed when the prefix declaration is gone — classifies NOTHING rather than silently stopping",
    parsePluginPrefixRule(`export const LOCAL_ONLY_COMMANDS = [];`) === null &&
      !pluginRuleClassifies("plugin:opener|open_url", null)
  );
  check(
    "fails closed when the exception LIST is gone — the mechanism may not vanish",
    parsePluginPrefixRule(`export const PLUGIN_COMMAND_PREFIX = "plugin:";`) === null
  );
  check(
    "notices plugin package specifiers behind static, dynamic, and re-export imports",
    (() => {
      const found = collectPluginPackageSpecifiers([
        parse(
          "frontend/src/lib/plugins.ts",
          `import { openUrl } from "@tauri-apps/plugin-opener";
           export { check } from "@tauri-apps/plugin-updater";
           const later = await import("@tauri-apps/plugin-dialog");
           import { invoke } from "@tauri-apps/api/core";
           const label = "@tauri-apps/plugin-opener";`
        ),
      ]);
      return (
        found.size === 3 &&
        found.has("@tauri-apps/plugin-opener") &&
        found.has("@tauri-apps/plugin-updater") &&
        found.has("@tauri-apps/plugin-dialog")
      );
    })()
  );
  check(
    "inventories a plugin package's own invoke literals with the same AST machinery",
    (() => {
      // The shape the real packages ship: a bare `invoke` import from the ALIASED core, then
      // literal `plugin:<ns>|<cmd>` names. This is why they ride our transport at all.
      const pluginFixture = parse(
        "frontend/node_modules/@tauri-apps/plugin-opener/dist-js/index.js",
        `import { invoke } from "@tauri-apps/api/core";
         async function openUrl(url) { await invoke("plugin:opener|open_url", { url }); }
         async function openPath(p) { await invoke("plugin:opener|open_path", { path: p }); }`
      );
      const names = collectInvokeCallSites(
        [pluginFixture],
        collectForwarders([pluginFixture])
      ).map((site) => site.command);
      return (
        names.length === 2 &&
        names.includes("plugin:opener|open_url") &&
        names.includes("plugin:opener|open_path")
      );
    })()
  );
  check(
    "still flags a DYNAMIC command inside a plugin package — an unenumerable name is the blindness itself",
    (() => {
      const dynamicPlugin = parse(
        "frontend/node_modules/@tauri-apps/plugin-x/dist-js/index.js",
        `import { invoke } from "@tauri-apps/api/core";
         async function go(op) { await invoke(\`plugin:x|\${op}\`, {}); }`
      );
      return collectInvokeCallSites(
        [dynamicPlugin],
        collectForwarders([dynamicPlugin])
      ).some((site) => site.command === null);
    })()
  );
  check(
    "reports an uninstalled plugin package rather than treating it as zero commands",
    pluginPackageEntryFile(
      path.join(repoRoot, "frontend"),
      "@tauri-apps/plugin-does-not-exist"
    ) === null
  );

  // --- P-11 exit criterion (batch 14): the permanent zero -------------------
  check(
    "P-11 passes only when BOTH the live count and the baseline are empty",
    p11ExitViolations([], new Set()).length === 0
  );
  check(
    "P-11 fails on a live unclassified name",
    p11ExitViolations(["newly_invoked"], new Set()).length === 1
  );
  check(
    "P-11 fails on a REGROWN baseline even when the live count is zero — the suppression case",
    p11ExitViolations([], new Set(["quietly_suppressed"])).length === 1
  );
  check(
    "P-11 reports both violations independently",
    p11ExitViolations(["a"], new Set(["b"])).length === 2
  );

  if (failures.length > 0) {
    console.error("FAIL: drift-scan self-test");
    failures.forEach((failure) => console.error(`  ${failure}`));
    process.exit(1);
  }
  console.log("PASS: drift-scan self-test (46 detector cases)");
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

// --- The Tauri plugin surface (Phase 2) ------------------------------------
//
// These files are NOT ours, but their invokes are on our transport: the `@tauri-apps/api/core`
// alias covers node_modules, so `@tauri-apps/plugin-opener`'s own `invoke("plugin:opener|open_url")`
// arrives at `lib/remote/invoke.ts` exactly like an app call site does. Scanning only
// `frontend/src` is what made the P-11 census structurally unable to see 77 import sites'
// worth of commands and still report "0 unclassified".
const pluginSpecifiers = [...collectPluginPackageSpecifiers(parsedFiles)].sort();
const pluginPackageFailures = [];
const pluginParsedFiles = [];
for (const specifier of pluginSpecifiers) {
  const entry = pluginPackageEntryFile(frontendRoot, specifier);
  if (entry === null) {
    pluginPackageFailures.push({
      file: `frontend/node_modules/${specifier}`,
      line: 0,
      detail:
        `imported by frontend/src but not installed — the plugin: command census cannot be ` +
        "computed, and an uncomputable census must not pass as an empty one",
    });
    continue;
  }
  pluginParsedFiles.push(parse(toRepoPath(entry), fs.readFileSync(entry, "utf8")));
}

const pluginCallSites = collectInvokeCallSites(
  pluginParsedFiles,
  collectForwarders(pluginParsedFiles)
);
const pluginCommands = [
  ...new Set(
    pluginCallSites
      .filter((site) => site.command !== null)
      .map((site) => site.command)
  ),
].sort();

const hardFailures = [
  ...callSites
    .filter((site) => site.command === null)
    .map((site) => ({
      file: site.file,
      line: site.line,
      detail:
        "dynamic invoke command expression — every production command name must be a literal (P-11)",
    })),
  ...pluginPackageFailures,
  // A dynamic command inside a plugin package is not a defect we can fix in this repo, but it
  // IS a name the census cannot enumerate — which is precisely the blindness Phase 2 closed.
  // Failing loudly on a dependency upgrade that introduces one beats quietly under-reporting.
  ...pluginCallSites
    .filter((site) => site.command === null)
    .map((site) => ({
      file: site.file,
      line: site.line,
      detail:
        "dynamic invoke command expression inside a Tauri plugin package — its command name " +
        "cannot be enumerated, so the plugin: census would silently under-report it. Pin the " +
        "plugin version, or classify the name explicitly in local-only-commands.ts",
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
const localOnlySource = fs.existsSync(localOnlyPath)
  ? fs.readFileSync(localOnlyPath, "utf8")
  : "";
const localOnly = parseLocalOnlyCommands(localOnlySource);
const pluginRule = parsePluginPrefixRule(localOnlySource);
if (pluginRule === null && pluginCommands.length > 0) {
  console.warn(
    "WARN: no `plugin:` prefix rule found in local-only-commands.ts; every plugin command " +
      "name will report as unclassified (Phase 2 routing policy missing or restructured)"
  );
}

const manifestClassified = fs.existsSync(manifestPath)
  ? parseManifestClassifications(JSON.parse(fs.readFileSync(manifestPath, "utf8")))
  : new Map();
if (manifestClassified.size === 0) {
  console.warn(
    "WARN: no manifest classifications found; the host-denied/deferred disposition is inert " +
      "(regenerate docs/generated/remote-commands.json)"
  );
}

// One inventory over both source sets. Plugin names are first-class members of it — a census
// that counted them separately could still report "0 unclassified" while a plugin family sat
// unrouted, which is the exact shape of the bug Phase 2 fixed.
const commands = [
  ...new Set([...callSites.map((site) => site.command), ...pluginCommands]),
].sort();
const unclassified = commands.filter(
  (command) =>
    !(registered ?? new Set()).has(command) &&
    !localOnly.has(command) &&
    !pluginRuleClassifies(command, pluginRule) &&
    !manifestClassified.has(command)
);

const manifestResolvedCount = commands.filter((command) =>
  manifestClassified.has(command)
).length;

const baseline = fs.existsSync(baselinePath)
  ? JSON.parse(fs.readFileSync(baselinePath, "utf8"))
  : { unclassifiedCommands: [] };
const baselineSet = new Set(baseline.unclassifiedCommands ?? []);

// ---------------------------------------------------------------------------------------
// P-11 EXIT CRITERION (PR 3.1-b batch 14). The ratchet reached zero; from here the gate is
// permanent and absolute.
//
// Until now this script only failed on ADDITIONS relative to a checked-in baseline, which is
// what a ratchet needs while it is still being driven down. That is no longer sufficient: a
// non-empty baseline is itself the failure, because `--update-baseline` would otherwise let a
// future change record new unclassified names "deliberately" and quietly regrow the list. The
// phase doc's P-11 requirement is zero unclassified names with zero suppressions, so both the
// live count and the recorded baseline are asserted empty, and the escape hatch is closed.
// ---------------------------------------------------------------------------------------
const P11_COMPLETE_NOTE =
  "P-11 COMPLETE (PR 3.1-b batch 14). Every invoke command name resolves to a registration, a " +
  "reason-coded local-only row, or a host-denied/deferred/audit-refused ledger row. This list " +
  "MUST stay empty — it is no longer a ratchet, it is a permanent zero.";

if (updateBaseline && unclassified.length > 0) {
  console.error(
    "FAIL: --update-baseline refused. P-11 is complete and the baseline is a permanent zero;\n" +
      `  it cannot be regrown to record ${unclassified.length} unclassified command(s):`
  );
  unclassified.forEach((command) => console.error(`    ${command}`));
  console.error(
    "  Resolve each one instead: register it on the host facade, add it to local-only-commands.ts\n" +
      "  with a reason, or classify it in capability_ledger.rs (Denied/Elevated/AUDIT_REFUSALS)\n" +
      "  and regenerate docs/generated/remote-commands.json."
  );
  process.exit(1);
}

if (updateBaseline) {
  fs.writeFileSync(
    baselinePath,
    `${JSON.stringify(
      {
        note: P11_COMPLETE_NOTE,
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
      `  ${added.length} command(s) are remote-registered by nothing, listed in ` +
        "local-only-commands.ts by nothing, and carry no host-denied/deferred ledger row:"
    );
    added.forEach((command) => console.error(`    ${command}`));
    console.error(
      "  Register them on the host facade, add them to local-only-commands.ts with a reason,\n" +
        "  classify them in capability_ledger.rs as Denied/Elevated (then regenerate the manifest),"
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

// The P-11 exit criterion, asserted on every run. `added`/`stale` above compare against the
// baseline; these compare against ZERO, which is the only value either may now hold.
for (const violation of p11ExitViolations(unclassified, baselineSet)) {
  console.error(violation);
  process.exit(1);
}

// The census statement: every invoke name the AST scan resolved has a reviewed disposition.
//
// Reported as a genuine PARTITION, in the precedence the resolver itself uses. The source
// sets overlap — a registered command also carries a ledger row, so it appears in
// `manifestClassified` too — and adding the raw sizes would exceed the name count and look like
// a coverage claim nobody made. Each name is counted exactly once, and the five buckets sum to
// `commands.length` by construction because `unclassified` is their complement.
const registeredSet = registered ?? new Set();
let registeredCount = 0;
let localOnlyCount = 0;
let pluginLocalCount = 0;
let manifestOnlyCount = 0;
for (const command of commands) {
  if (registeredSet.has(command)) registeredCount += 1;
  else if (localOnly.has(command)) localOnlyCount += 1;
  else if (pluginRuleClassifies(command, pluginRule)) pluginLocalCount += 1;
  else if (manifestClassified.has(command)) manifestOnlyCount += 1;
}
const dispositioned =
  registeredCount +
  localOnlyCount +
  pluginLocalCount +
  manifestOnlyCount +
  unclassified.length;
if (dispositioned !== commands.length) {
  console.error(
    `FAIL: P-11 census does not partition — ${dispositioned} dispositions for ` +
      `${commands.length} names. The five buckets must be exhaustive and disjoint.`
  );
  process.exit(1);
}

console.log(
  `PASS: remote transport drift — ${commands.length} invoke command name(s), 0 dynamic, ` +
    `0 seam bypasses; ${manifestResolvedCount} manifest-classified; ` +
    `${unclassified.length} unclassified (P-11 COMPLETE — permanent zero).`
);
console.log(
  `      P-11 census: all ${commands.length} names have a reviewed disposition — ` +
    `${registeredCount} remote-registered, ${localOnlyCount} reason-coded local-only, ` +
    `${pluginLocalCount} plugin-local (prefix rule), ` +
    `${manifestOnlyCount} manifest-classified only, 0 unclassified, 0 suppressions.`
);
console.log(
  `      Tauri plugin surface: ${pluginCommands.length} plugin: command name(s) across ` +
    `${pluginSpecifiers.length} imported @tauri-apps/plugin-* package(s), ` +
    `${pluginRule?.exceptions.size ?? 0} reviewed host-targeted exception(s).`
);
