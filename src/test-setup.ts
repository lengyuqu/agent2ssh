import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// jsdom sometimes lacks localStorage (e.g. opaque origin), which breaks
// I18nProvider on mount. Provide an in-memory shim when it's missing so the
// suite runs green regardless of the jsdom build in use.
if (typeof globalThis.localStorage === "undefined") {
  const store = new Map<string, string>();
  globalThis.localStorage = {
    get length() {
      return store.size;
    },
    clear: () => store.clear(),
    getItem: (key: string) => (store.has(key) ? (store.get(key) as string) : null),
    key: (index: number) => Array.from(store.keys())[index] ?? null,
    removeItem: (key: string) => {
      store.delete(key);
    },
    setItem: (key: string, value: string) => {
      store.set(key, String(value));
    },
  } as Storage;
}

// Vitest runs without globals, so Testing Library's automatic DOM cleanup
// never registers. Unmount rendered trees after every test explicitly.
afterEach(() => {
  cleanup();
});
