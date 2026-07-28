import type { PersistStorage, StorageValue } from "zustand/middleware";

import { getTransportEnvironmentId, LOCAL_ENVIRONMENT_ID } from "./active-environment";
import { STORE_ISOLATION_INVENTORY } from "./store-isolation-inventory";

const persistedSpecs = new Map(
  STORE_ISOLATION_INVENTORY.flatMap((entry) =>
    entry.persisted ? [[entry.persisted.storageName, entry.persisted] as const] : [],
  ),
);

let suppressionDepth = 0;

export function runSuppressed<T>(operation: () => T): T {
  suppressionDepth += 1;
  try {
    return operation();
  } finally {
    suppressionDepth -= 1;
  }
}

function parse<S>(value: string | null): StorageValue<S> | null {
  if (value === null) return null;
  return JSON.parse(value) as StorageValue<S>;
}

function pick(source: unknown, fields: readonly string[]): Record<string, unknown> {
  if (!source || typeof source !== "object") return {};
  const record = source as Record<string, unknown>;
  return Object.fromEntries(fields.filter((field) => field in record).map((field) => [field, record[field]]));
}

export function createEnvScopedStorage<S>(storageName: string): PersistStorage<S> {
  const spec = persistedSpecs.get(storageName);
  if (!spec) throw new Error(`Unknown env-scoped persisted store: ${storageName}`);

  return {
    getItem: () => {
      const storage = globalThis.localStorage;
      if (!storage) return null;
      const environmentId = getTransportEnvironmentId();
      const shared = parse<S>(storage.getItem(storageName));
      if (environmentId === LOCAL_ENVIRONMENT_ID) return shared;
      const scoped = parse<S>(storage.getItem(`${storageName}:${environmentId}`));
      if (!shared && !scoped) return null;

      const versions = [shared?.version, scoped?.version].filter(
        (version): version is number => typeof version === "number",
      );
      // The oldest component version wins so persist migration runs if either slice is stale.
      const version = versions.length > 0 ? Math.min(...versions) : undefined;
      return {
        state: {
          ...pick(shared?.state, spec.globalFields),
          ...pick(scoped?.state, spec.envFields),
        } as S,
        ...(version !== undefined ? { version } : {}),
      };
    },
    setItem: (_name, value) => {
      const storage = globalThis.localStorage;
      if (!storage || suppressionDepth > 0) return;
      const environmentId = getTransportEnvironmentId();
      if (environmentId === LOCAL_ENVIRONMENT_ID) {
        storage.setItem(storageName, JSON.stringify(value));
        return;
      }
      const shared = parse<S>(storage.getItem(storageName));
      storage.setItem(
        storageName,
        JSON.stringify({
          state: {
            ...pick(shared?.state, spec.envFields),
            ...pick(value.state, spec.globalFields),
          },
          version: shared?.version ?? value.version,
        }),
      );
      storage.setItem(
        `${storageName}:${environmentId}`,
        JSON.stringify({ state: pick(value.state, spec.envFields), version: value.version }),
      );
    },
    removeItem: () => {
      const storage = globalThis.localStorage;
      if (!storage || suppressionDepth > 0) return;
      const environmentId = getTransportEnvironmentId();
      // Remote removal clears only that environment; shared local/global data is conservative.
      storage.removeItem(
        environmentId === LOCAL_ENVIRONMENT_ID ? storageName : `${storageName}:${environmentId}`,
      );
    },
  };
}
