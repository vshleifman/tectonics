import {
  defaultSimOptions,
  defaultTuning,
  type SimOptions,
  simOptions,
  type Tuning,
  tuning,
} from "../sim/tuning";
import { MAX_LEVEL, MIN_LEVEL } from "./controls";
import type {
  PlateOverlay,
  Settings,
  SettingsStore,
  ViewMode,
} from "./settingsStore";

/**
 * Browser storage for what the user has set: the UI settings, and the tuning
 * and modelling options they have moved off their defaults. Storage can be
 * missing, full or hold something written by an older version of the app, so
 * every read is checked key by key and every failure is ignored, leaving the
 * defaults.
 */

const SETTINGS_KEY = "tectonics.settings";
const TUNING_KEY = "tectonics.tuning";

/** Writes this long after the last change are coalesced into one. */
const WRITE_DELAY_MS = 300;

/** Settings that always start afresh: a reload never resumes running. */
const NOT_KEPT: readonly (keyof Settings)[] = ["running", "seedMode"];

/** The values each choice-valued setting may hold. */
const CHOICES: Partial<Record<keyof Settings, readonly string[]>> = {
  projection: ["sphere", "mercator"],
  elevationStyle: ["absolute", "land"],
  viewMode: [
    "colour",
    "data",
    "crust",
    "density",
    "lithosphere",
    "sediment",
    "heat-flow",
    "mantle-temp",
    "slab-graveyard",
    "dyn-topo",
    "boundary",
  ] satisfies ViewMode[],
  plateOverlay: ["off", "lines", "colours"] satisfies PlateOverlay[],
};

const readJson = (key: string): Record<string, unknown> | null => {
  try {
    const text = localStorage.getItem(key);
    if (text === null) return null;
    const value: unknown = JSON.parse(text);
    return typeof value === "object" && value !== null && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
};

const writeJson = (key: string, value: object): void => {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Storage is unavailable or full; the app works without it.
  }
};

/** Whether `value` is something setting `key` may hold, going by the default's type. */
const fits = (
  key: keyof Settings,
  value: unknown,
  fallback: unknown,
): boolean => {
  if (typeof value !== typeof fallback) return false;
  if (typeof value === "number") return Number.isFinite(value);
  const choices = CHOICES[key];
  return choices === undefined || choices.includes(value as string);
};

/** `defaults` with whatever valid settings were stored over them. */
export const loadSettings = (defaults: Settings): Settings => {
  const settings = { ...defaults };
  const stored = readJson(SETTINGS_KEY);
  if (!stored) return settings;
  const assign = <K extends keyof Settings>(
    key: K,
    value: Settings[K],
  ): void => {
    settings[key] = value;
  };
  for (const key of Object.keys(defaults) as (keyof Settings)[]) {
    if (NOT_KEPT.includes(key)) continue;
    const value = stored[key];
    if (fits(key, value, defaults[key]))
      assign(key, value as Settings[typeof key]);
  }
  settings.level = Math.round(
    Math.min(MAX_LEVEL, Math.max(MIN_LEVEL, settings.level)),
  );
  return settings;
};

/** The stored tuning and options laid over the live ones, which hold the defaults until now. */
export const loadTuning = (): void => {
  const stored = readJson(TUNING_KEY);
  if (!stored) return;
  const apply = <T extends object>(
    live: T,
    defaults: T,
    saved: unknown,
  ): void => {
    if (typeof saved !== "object" || saved === null) return;
    const patch = saved as Record<string, unknown>;
    const target = live as Record<string, unknown>;
    for (const key of Object.keys(defaults)) {
      const value = patch[key];
      if (typeof value !== typeof (defaults as Record<string, unknown>)[key])
        continue;
      if (typeof value === "number" && !Number.isFinite(value)) continue;
      target[key] = value;
    }
  };
  apply<Tuning>(tuning, defaultTuning, stored.tuning);
  apply<SimOptions>(simOptions, defaultSimOptions, stored.options);
};

/** The entries of `live` that differ from `defaults`. */
const changed = <T extends object>(live: T, defaults: T): Partial<T> => {
  const out: Record<string, unknown> = {};
  for (const key of Object.keys(defaults) as (keyof T & string)[]) {
    if (live[key] !== defaults[key]) out[key] = live[key];
  }
  return out as Partial<T>;
};

export interface Persistence {
  /** The live tuning or modelling options have changed. */
  tuningChanged: () => void;
  /** Write anything pending now, for when the page is going away. */
  flush: () => void;
}

/**
 * Keep storage in step with the settings store and the live tuning. Only what
 * differs from the defaults is kept for tuning, so a default changed in code
 * reaches everything the user has not moved.
 */
export const persistSettings = (store: SettingsStore): Persistence => {
  let settingsTimer: ReturnType<typeof setTimeout> | null = null;
  let tuningTimer: ReturnType<typeof setTimeout> | null = null;

  const writeSettings = (): void => {
    settingsTimer = null;
    const kept: Record<string, unknown> = { ...store.getAll() };
    for (const key of NOT_KEPT) delete kept[key];
    writeJson(SETTINGS_KEY, kept);
  };
  const writeTuning = (): void => {
    tuningTimer = null;
    writeJson(TUNING_KEY, {
      tuning: changed(tuning, defaultTuning),
      options: changed(simOptions, defaultSimOptions),
    });
  };

  store.subscribe(() => {
    settingsTimer ??= setTimeout(writeSettings, WRITE_DELAY_MS);
  });

  return {
    tuningChanged: () => {
      tuningTimer ??= setTimeout(writeTuning, WRITE_DELAY_MS);
    },
    flush: () => {
      if (settingsTimer !== null) {
        clearTimeout(settingsTimer);
        writeSettings();
      }
      if (tuningTimer !== null) {
        clearTimeout(tuningTimer);
        writeTuning();
      }
    },
  };
};
