import { lazy, type ComponentType, type LazyExoticComponent } from "react";
import { isModuleLoadError } from "@/lib/module-load-error";

const RETRY_DELAY_MS = 350;

/** Wraps a lazy module factory with one delayed retry for transport failures. */
// `ComponentType<any>` mirrors React's own constraint on lazy/LazyExoticComponent;
// narrowing it here rejects components with props or class statics.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function withRetry<T extends ComponentType<any>>(
  factory: () => Promise<{ default: T }>,
): () => Promise<{ default: T }> {
  return async () => {
    try {
      return await factory();
    } catch (error) {
      if (!isModuleLoadError(error)) throw error;

      await delay(RETRY_DELAY_MS);
      return factory();
    }
  };
}

/** React.lazy with one delayed retry for transient dynamic-import failures. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function lazyWithRetry<T extends ComponentType<any>>(
  factory: () => Promise<{ default: T }>,
): LazyExoticComponent<T> {
  return lazy(withRetry(factory));
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}
