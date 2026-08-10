/**
 * The alias is the whole A-3 mechanism: ~116 importers switch transport without a
 * single call-site edit BECAUSE `@tauri-apps/api/core` resolves to the wrapper. That
 * makes the alias itself load-bearing behaviour, not configuration, so it is asserted
 * here — including the two ways it can silently regress: web mode losing its mock, and
 * `lib/tauri.ts` drifting to an un-aliased import specifier.
 */

import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

import viteConfig from "../../../vite.config";

const frontendRoot = path.resolve(__dirname, "../../..");

type ResolvedAliases = Record<string, string>;

async function aliasesFor(mode: string): Promise<ResolvedAliases> {
  const factory = viteConfig as unknown as (env: {
    mode: string;
    command: "serve" | "build";
  }) => Promise<{ resolve?: { alias?: ResolvedAliases } }>;
  const config = await factory({ mode, command: "serve" });
  return config.resolve?.alias ?? {};
}

describe("vite transport alias", () => {
  it("points @tauri-apps/api/core at the project-owned wrapper in native mode", async () => {
    const alias = await aliasesFor("development");
    expect(alias["@tauri-apps/api/core"]).toBe(
      path.resolve(frontendRoot, "src/lib/remote/invoke.ts")
    );
  });

  it("gives the wrapper an un-aliased specifier for the real core", async () => {
    const alias = await aliasesFor("development");
    expect(alias["#tauri-core-primitive"]).toBe(
      path.resolve(frontendRoot, "node_modules/@tauri-apps/api/core.js")
    );
  });

  it("keeps the web mock winning in web mode", async () => {
    const alias = await aliasesFor("web");
    expect(alias["@tauri-apps/api/core"]).toBe(
      path.resolve(frontendRoot, "src/mocks/tauri-api-core.ts")
    );
  });

  it("keeps the other web-mode Tauri mocks untouched", async () => {
    const alias = await aliasesFor("web");
    expect(alias["@tauri-apps/api/event"]).toBe(
      path.resolve(frontendRoot, "src/mocks/tauri-api-event.ts")
    );
    expect(alias["@tauri-apps/api/webview"]).toBe(
      path.resolve(frontendRoot, "src/mocks/tauri-api-webview.ts")
    );
  });
});

describe("the alias actually covers typedInvoke (A-3)", () => {
  // `typedInvoke` moved out of the `@/lib/tauri` barrel into its own module so that
  // importing it no longer drags the barrel's whole API graph along. The invariant is
  // unchanged and now asserted where the function actually lives.
  it("typed-invoke.ts imports invoke from the aliased specifier", () => {
    const source = fs.readFileSync(
      path.resolve(frontendRoot, "src/lib/typed-invoke.ts"),
      "utf8"
    );
    expect(source).toMatch(
      /import\s*\{\s*invoke\s*\}\s*from\s*["']@tauri-apps\/api\/core["']/
    );
    // A deeper specifier would resolve past the alias and silently strand
    // typedInvoke on local IPC while every other caller went remote.
    expect(source).not.toMatch(/@tauri-apps\/api\/core\.js/);
    expect(source).not.toMatch(/#tauri-core-primitive/);
  });

  it("keeps the barrel re-exporting typedInvoke for existing call sites", () => {
    const source = fs.readFileSync(
      path.resolve(frontendRoot, "src/lib/tauri.ts"),
      "utf8"
    );
    expect(source).toMatch(/export\s*\{[\s\S]*typedInvoke[\s\S]*\}\s*from\s*["']\.\/typed-invoke["']/);
  });
});

describe("the primitive specifier stays confined to the transport modules", () => {
  // Same scope as scripts/check-remote-transport-drift.mjs, which is the CI
  // authority: production modules only. A test file may mock the primitive to put a
  // spy underneath the wrapper — that is the point of having a private specifier.
  it("is imported only from src/lib/remote in production code", () => {
    const offenders: string[] = [];
    const walk = (directory: string) => {
      for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
        const entryPath = path.join(directory, entry.name);
        if (entry.isDirectory()) {
          walk(entryPath);
          continue;
        }
        if (!/\.[cm]?[jt]sx?$/.test(entry.name)) continue;
        if (/\.(?:test|spec)\.[cm]?[jt]sx?$/.test(entry.name)) continue;
        const relative = path
          .relative(frontendRoot, entryPath)
          .split(path.sep)
          .join("/");
        if (relative.startsWith("src/lib/remote/")) continue;
        if (fs.readFileSync(entryPath, "utf8").includes("#tauri-core-primitive")) {
          offenders.push(relative);
        }
      }
    };
    walk(path.resolve(frontendRoot, "src"));
    expect(offenders).toEqual([]);
  });
});
