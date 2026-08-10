import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

import { useAgentTerminalStore } from "@/components/agents/agentTerminalStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useProjectStore } from "@/stores/projectStore";
import { useTicketingStore } from "@/stores/ticketingStore";
import { STORE_ISOLATION_INVENTORY } from "./store-isolation-inventory";

/**
 * Every local binding a module imports from ANY zustand entry point — `zustand`,
 * `zustand/vanilla`, `zustand/traditional`, and aliased forms
 * (`import { create as createStore }`). Matching the binding rather than the literal
 * `create<` is what keeps an untyped `create(persist(...))`, a
 * `createWithEqualityFn`, or an alias from adding an env-owned store that skips this
 * inventory — and therefore skips the reset-on-switch funnel — with green CI.
 */
function zustandCreateBindings(source: string): string[] {
  const bindings: string[] = [];
  const imports = source.matchAll(
    /import\s+([^;]+?)\s+from\s+["']zustand(?:\/[\w-]+)*["']/g
  );
  for (const [, clause] of imports) {
    for (const [, imported, alias] of (clause ?? "").matchAll(
      /(?:^|[{,\s])([A-Za-z_$][\w$]*)(?:\s+as\s+([A-Za-z_$][\w$]*))?/g
    )) {
      const local = alias ?? imported;
      if (local !== undefined && /^create/i.test(imported ?? "")) {
        bindings.push(local);
      }
    }
  }
  return bindings;
}

function createsStore(source: string): boolean {
  return zustandCreateBindings(source).some((binding) =>
    new RegExp(`\\b${binding}\\s*[<(]`).test(source)
  );
}

function findStores(root: string): string[] {
  const found: string[] = [];
  const walk = (directory: string) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (entry.name === "node_modules" || entry.name === "__tests__") continue;
      const path = join(directory, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (!/\.test\./.test(entry.name)) {
        const source = readFileSync(path, "utf8");
        if (createsStore(source)) {
          found.push(relative(join(root, ".."), path).replaceAll("\\", "/"));
        }
      }
    }
  };
  walk(root);
  return found.sort();
}

describe("store isolation inventory", () => {
  it("exactly enumerates every Zustand create site", () => {
    expect(STORE_ISOLATION_INVENTORY.map((entry) => entry.modulePath).sort()).toEqual(
      findStores(join(process.cwd(), "src")),
    );
  });

  it("detects create sites the old `create<` scan was blind to", () => {
    expect(
      createsStore(`import { create } from "zustand";\nexport const s = create(persist(f, o));`)
    ).toBe(true);
    expect(
      createsStore(
        `import { createWithEqualityFn } from "zustand/traditional";\nconst s = createWithEqualityFn(f);`
      )
    ).toBe(true);
    expect(
      createsStore(
        `import { create as createStore } from "zustand/vanilla";\nconst s = createStore(f);`
      )
    ).toBe(true);
    expect(createsStore(`import { persist } from "zustand/middleware";`)).toBe(false);
    expect(createsStore(`const create = other();\ncreate(1);`)).toBe(false);
  });

  it("matches every persisted partialize contract with disjoint fields", () => {
    const stores = new Map<string, typeof useProjectStore>([
      ["ralphx-project-store", useProjectStore],
      ["ralphx-agent-session-store", useAgentSessionStore as typeof useProjectStore],
      ["ralphx-ticketing-store", useTicketingStore as typeof useProjectStore],
      ["ralphx-agent-terminal-ui", useAgentTerminalStore as typeof useProjectStore],
    ]);
    for (const entry of STORE_ISOLATION_INVENTORY) {
      if (!entry.persisted) continue;
      const store = stores.get(entry.persisted.storageName);
      expect(store, entry.storeName).toBeDefined();
      const partialize = store!.persist.getOptions().partialize;
      expect(partialize).toBeTypeOf("function");
      const actual = Object.keys(partialize!(store!.getState())).sort();
      const expected = [...entry.persisted.envFields, ...entry.persisted.globalFields].sort();
      expect(actual, entry.storeName).toEqual(expected);
      expect(entry.persisted.envFields.filter((field) => entry.persisted!.globalFields.includes(field))).toEqual([]);
    }
  });

  it("has valid classifications, rationales, and exactly the audited infrastructure stores", () => {
    const allowed = ["env-owned", "global", "mixed", "infrastructure"];
    expect(STORE_ISOLATION_INVENTORY.every((entry) => entry.rationale.trim().length > 0)).toBe(true);
    expect(STORE_ISOLATION_INVENTORY.every((entry) => allowed.includes(entry.classification))).toBe(true);
    // Pin the exact set, not a count: environment identity and the connection journal are
    // the only stores allowed to sit outside the env-isolation reset contract.
    expect(
      STORE_ISOLATION_INVENTORY.filter((entry) => entry.classification === "infrastructure")
        .map((entry) => entry.storeName)
        .sort(),
    ).toEqual(["useEnvironmentStore", "useRemoteConnectionJournalStore"]);
  });
});
