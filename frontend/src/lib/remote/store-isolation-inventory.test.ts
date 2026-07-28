import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

import { useAgentTerminalStore } from "@/components/agents/agentTerminalStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useProjectStore } from "@/stores/projectStore";
import { useTicketingStore } from "@/stores/ticketingStore";
import { STORE_ISOLATION_INVENTORY } from "./store-isolation-inventory";

function findStores(root: string): string[] {
  const found: string[] = [];
  const walk = (directory: string) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (entry.name === "node_modules" || entry.name === "__tests__") continue;
      const path = join(directory, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (!/\.test\./.test(entry.name)) {
        const source = readFileSync(path, "utf8");
        if (/from ["']zustand["']/.test(source) && /\bcreate\s*</.test(source)) {
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

  it("has valid classifications, rationales, and one infrastructure store", () => {
    const allowed = ["env-owned", "global", "mixed", "infrastructure"];
    expect(STORE_ISOLATION_INVENTORY.every((entry) => entry.rationale.trim().length > 0)).toBe(true);
    expect(STORE_ISOLATION_INVENTORY.every((entry) => allowed.includes(entry.classification))).toBe(true);
    expect(STORE_ISOLATION_INVENTORY.filter((entry) => entry.classification === "infrastructure")).toHaveLength(1);
  });
});
