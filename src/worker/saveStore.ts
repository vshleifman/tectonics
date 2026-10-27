/**
 * The one saved world, in IndexedDB: the simulation's own bytes plus the fault
 * network the worker keeps beside it. IndexedDB rather than localStorage
 * because the world is binary and runs to tens of megabytes. Every call
 * swallows storage failures (private windows, blocked site data) and reports
 * them as "no save", since the app works fine without one.
 */

const DATABASE = "tectonics";
const STORE = "saves";
const KEY = "autosave";

export interface SavedWorld {
  level: number;
  tick: number;
  /** 1 where an arc is cracked, from the worker's fault network. */
  cracked: Uint8Array;
  seeds: number[];
  /** `SimWorld.saveState()`. */
  sim: Uint8Array;
}

const open = (): Promise<IDBDatabase> =>
  new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });

/** Run one request against the store and close the database behind it. */
const withStore = async <T>(
  mode: IDBTransactionMode,
  run: (store: IDBObjectStore) => IDBRequest<T>,
): Promise<T> => {
  const db = await open();
  try {
    return await new Promise<T>((resolve, reject) => {
      const transaction = db.transaction(STORE, mode);
      const request = run(transaction.objectStore(STORE));
      transaction.oncomplete = () => resolve(request.result);
      transaction.onerror = () => reject(transaction.error);
      transaction.onabort = () => reject(transaction.error);
    });
  } finally {
    db.close();
  }
};

export const readSave = async (): Promise<SavedWorld | null> => {
  try {
    const saved = await withStore<SavedWorld | undefined>("readonly", (s) =>
      s.get(KEY),
    );
    return saved ?? null;
  } catch (error) {
    console.warn("Could not read the saved world.", error);
    return null;
  }
};

/** Resolves false, rather than throwing, when the world could not be stored. */
export const writeSave = async (save: SavedWorld): Promise<boolean> => {
  try {
    await withStore("readwrite", (s) => s.put(save, KEY));
    return true;
  } catch (error) {
    console.warn("Could not save the world.", error);
    return false;
  }
};

export const clearSave = async (): Promise<void> => {
  try {
    await withStore("readwrite", (s) => s.delete(KEY));
  } catch (error) {
    console.warn("Could not clear the saved world.", error);
  }
};
