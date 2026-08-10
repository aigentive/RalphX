import { runSuppressed } from "./env-scoped-storage";

export interface EnvIsolatedStoreRegistration {
  readonly name: string;
  readonly reset?: () => void;
  readonly rehydrate?: () => void;
}

const registrations = new Map<string, EnvIsolatedStoreRegistration>();

export function registerEnvIsolatedStore(registration: EnvIsolatedStoreRegistration): void {
  registrations.set(registration.name, registration);
}

export function onEnvironmentSwitched(): void {
  runSuppressed(() => {
    for (const registration of registrations.values()) registration.reset?.();
    for (const registration of registrations.values()) registration.rehydrate?.();
  });
}

export function resetEnvStateIsolationForTests(): void {
  registrations.clear();
}
