// In-memory localStorage mock for unit tests.
//
// Node 22+ has a built-in localStorage global that lacks standard methods
// (getItem, setItem, removeItem, clear). This mock provides a proper
// implementation backed by a Map.

export class MockLocalStorage implements Storage {
  private store = new Map<string, string>();

  get length(): number {
    return this.store.size;
  }

  clear(): void {
    this.store.clear();
  }

  getItem(key: string): string | null {
    return this.store.get(key) ?? null;
  }

  key(index: number): string | null {
    const keys = [...this.store.keys()];
    return keys[index] ?? null;
  }

  removeItem(key: string): void {
    this.store.delete(key);
  }

  setItem(key: string, value: string): void {
    this.store.set(key, value);
  }

  // Allow bracket-syntax access for Web Storage spec compliance.
  [name: string]: unknown;
}

/** Install a fresh MockLocalStorage on globalThis. Returns cleanup function. */
export function installMockLocalStorage(): () => void {
  const original = globalThis.localStorage;
  const mock = new MockLocalStorage();
  Object.defineProperty(globalThis, "localStorage", {
    value: mock,
    writable: true,
    configurable: true,
  });
  return () => {
    Object.defineProperty(globalThis, "localStorage", {
      value: original,
      writable: true,
      configurable: true,
    });
  };
}
