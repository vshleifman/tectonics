import type { Projection } from "../render/projection";

/**
 * How cells render: elevation colour ("colour"), greyscale with per-cell
 * id/elevation labels ("data"), crust type ("crust"), density ramp ("density"),
 * or boundary type ("boundary"). Plates are no longer a base view mode — they
 * are an independently-toggled overlay (see `showPlates`).
 */
export type ViewMode = "colour" | "data" | "crust" | "density" | "boundary";

/** Every user-tunable UI setting. The store below is the sole owner of these. */
export interface Settings {
  level: number;
  seaLevel: number;
  autoRotate: boolean;
  projection: Projection;
  viewMode: ViewMode;
  seedMode: boolean;
  branchesPerSeed: number;
  showVelocities: boolean;
  showPlates: boolean;
  running: boolean;
}

/** Listener fired after a `set`, told which keys actually changed. */
type Listener = (
  changed: ReadonlySet<keyof Settings>,
  state: Readonly<Settings>,
) => void;

/**
 * Single source of truth for UI settings: read with `get`/`getAll`, mutate with
 * `set` (which only notifies on real changes), and react with `subscribe` or the
 * per-key `on` convenience.
 */
export interface SettingsStore {
  get: <K extends keyof Settings>(key: K) => Settings[K];
  getAll: () => Readonly<Settings>;
  set: (patch: Partial<Settings>) => void;
  subscribe: (fn: Listener) => () => void;
  on: <K extends keyof Settings>(
    key: K,
    fn: (value: Settings[K]) => void,
  ) => () => void;
}

/**
 * Tiny dependency-free observable store. `set` applies only the keys whose value
 * actually differs and notifies subscribers with that changed set; if nothing
 * differs it is a no-op, so writing the current value back (e.g. from a DOM echo)
 * cannot trigger a feedback loop.
 */
export const createSettingsStore = (initial: Settings): SettingsStore => {
  const state: Settings = { ...initial };
  const listeners = new Set<Listener>();

  // Indexed writes across a union of keys defeat TS's narrowing, so funnel the
  // assignment through one generic helper that pins a single concrete key type.
  const assign = <K extends keyof Settings>(
    key: K,
    value: Settings[K],
  ): void => {
    state[key] = value;
  };

  const set = (patch: Partial<Settings>): void => {
    const changed = new Set<keyof Settings>();
    for (const key of Object.keys(patch) as (keyof Settings)[]) {
      const value = patch[key];
      if (value !== undefined && value !== state[key]) {
        assign(key, value as Settings[typeof key]);
        changed.add(key);
      }
    }
    if (changed.size === 0) return;
    const snapshot = { ...state };
    for (const listener of listeners) listener(changed, snapshot);
  };

  const subscribe = (fn: Listener): (() => void) => {
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  };

  const on = <K extends keyof Settings>(
    key: K,
    fn: (value: Settings[K]) => void,
  ): (() => void) =>
    subscribe((changed, snapshot) => {
      if (changed.has(key)) fn(snapshot[key]);
    });

  return {
    get: <K extends keyof Settings>(key: K) => state[key],
    getAll: () => state,
    set,
    subscribe,
    on,
  };
};
