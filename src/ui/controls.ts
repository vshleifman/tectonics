import { clamp } from "../math/scalar";
import type { Projection } from "../render/projection";
import type { SettingsStore, ViewMode } from "./settingsStore";

/** Imperative handle for pushing live sim readouts back into the panel. */
export interface Controls {
  setCellCount: (count: number) => void;
  setPlateCount: (count: number) => void;
  setFps: (fps: number) => void;
  setMassReadout: (liveMass: number, drift: number) => void;
  setNote: (text: string) => void;
}

const MIN_LEVEL = 2;
const MAX_LEVEL = 8;
const MIN_SEA_LEVEL = -8;
const MAX_SEA_LEVEL = 8;
const MIN_BRANCHES = 1;
const MAX_BRANCHES = 5;

/**
 * Minimal framework-free overlay: subdivision-level and sea-level sliders, an
 * auto-rotate toggle, plus live FPS and cell-count readouts. The settings store
 * is the single source of truth: inputs write to it, and the panel reflects any
 * change back (including programmatic ones). Clear-faults is an action, not a
 * setting, so it stays a dedicated callback.
 */
export const createControls = (
  parent: HTMLElement,
  store: SettingsStore,
  onClearFaults: () => void,
): Controls => {
  const initial = store.getAll();

  const panel = document.createElement("div");
  Object.assign(panel.style, {
    position: "fixed",
    top: "12px",
    left: "12px",
    padding: "12px 14px",
    background: "rgba(10, 14, 22, 0.78)",
    border: "1px solid rgba(120, 140, 180, 0.25)",
    borderRadius: "10px",
    color: "#e6ecf5",
    font: "13px/1.4 ui-sans-serif, system-ui, sans-serif",
    backdropFilter: "blur(6px)",
    userSelect: "none",
    minWidth: "220px",
  } satisfies Partial<CSSStyleDeclaration>);

  const title = document.createElement("div");
  title.textContent = "Tectonic World-Builder";
  Object.assign(title.style, {
    fontWeight: "600",
    marginBottom: "10px",
    letterSpacing: "0.02em",
  } satisfies Partial<CSSStyleDeclaration>);

  // Subdivision-level slider.
  const levelLabel = makeLabel();
  const levelSlider = makeSlider(MIN_LEVEL, MAX_LEVEL, 1, initial.level);

  // Sea-level slider.
  const seaLabel = makeLabel();
  const seaSlider = makeSlider(
    MIN_SEA_LEVEL,
    MAX_SEA_LEVEL,
    0.5,
    initial.seaLevel,
  );

  // Auto-rotate toggle.
  const rotateToggle = makeToggle("Auto-rotate", initial.autoRotate);

  // Projection selector (sphere / Mercator map).
  const projectionLabel = makeLabel();
  projectionLabel.textContent = "Projection";
  const projectionSelect = makeSelect(
    [
      { value: "sphere", label: "Sphere" },
      { value: "mercator", label: "Mercator map" },
    ],
    initial.projection,
  );

  // View-mode selector (colour / data / plate).
  const viewLabel = makeLabel();
  viewLabel.textContent = "View";
  const viewSelect = makeSelect(
    [
      { value: "colour", label: "Elevation colour" },
      { value: "data", label: "Data (all properties)" },
      { value: "plate", label: "Plates" },
      { value: "crust", label: "Crust type" },
      { value: "density", label: "Density" },
      { value: "boundary", label: "Boundaries" },
    ],
    initial.viewMode,
  );

  // Seed-placement mode: clicks drop plate seeds instead of orbiting.
  const seedToggle = makeToggle(
    "Place plate seeds (click globe)",
    initial.seedMode,
  );

  // Plate-velocity arrows overlay toggle.
  const velocityToggle = makeToggle(
    "Show plate velocities",
    initial.showVelocities,
  );

  // Branches-per-seed slider (how many faults each seed fans out).
  const branchesLabel = makeLabel();
  const branchesSlider = makeSlider(
    MIN_BRANCHES,
    MAX_BRANCHES,
    1,
    initial.branchesPerSeed,
  );

  // Run-simulation toggle: advances plate motion while checked.
  const runToggle = makeToggle("Run simulation", initial.running);

  // Clear-faults button.
  const clearButton = makeButton("Clear faults");

  const readout = document.createElement("div");
  Object.assign(readout.style, {
    display: "flex",
    justifyContent: "space-between",
    gap: "12px",
    fontVariantNumeric: "tabular-nums",
    opacity: "0.95",
  } satisfies Partial<CSSStyleDeclaration>);

  const fpsEl = document.createElement("span");
  const cellEl = document.createElement("span");
  const plateEl = document.createElement("span");
  readout.append(fpsEl, cellEl, plateEl);

  // Conservation budget: live crust mass and its drift from the initial total.
  const massEl = document.createElement("div");
  Object.assign(massEl.style, {
    marginTop: "6px",
    fontVariantNumeric: "tabular-nums",
    opacity: "0.95",
  } satisfies Partial<CSSStyleDeclaration>);

  const note = document.createElement("div");
  Object.assign(note.style, {
    marginTop: "6px",
    opacity: "0.7",
    fontSize: "11px",
    minHeight: "13px",
  } satisfies Partial<CSSStyleDeclaration>);

  const renderLevelLabel = (): void => {
    levelLabel.textContent = `Subdivision level: ${store.get("level")}`;
  };
  const renderSeaLabel = (): void => {
    seaLabel.textContent = `Sea level: ${store.get("seaLevel").toFixed(1)}`;
  };
  const renderBranchesLabel = (): void => {
    branchesLabel.textContent = `Branches per seed: ${store.get(
      "branchesPerSeed",
    )}`;
  };
  renderLevelLabel();
  renderSeaLabel();
  renderBranchesLabel();
  fpsEl.textContent = "FPS: --";
  cellEl.textContent = "Cells: --";
  plateEl.textContent = "Plates: --";
  massEl.textContent = "Mass: --";

  levelSlider.addEventListener("input", () => {
    store.set({
      level: clamp(Math.round(Number(levelSlider.value)), MIN_LEVEL, MAX_LEVEL),
    });
  });

  seaSlider.addEventListener("input", () => {
    store.set({
      seaLevel: clamp(Number(seaSlider.value), MIN_SEA_LEVEL, MAX_SEA_LEVEL),
    });
  });

  rotateToggle.input.addEventListener("change", () => {
    store.set({ autoRotate: rotateToggle.input.checked });
  });

  projectionSelect.addEventListener("change", () => {
    store.set({ projection: projectionSelect.value as Projection });
  });

  viewSelect.addEventListener("change", () => {
    store.set({ viewMode: viewSelect.value as ViewMode });
  });

  seedToggle.input.addEventListener("change", () => {
    store.set({ seedMode: seedToggle.input.checked });
  });

  velocityToggle.input.addEventListener("change", () => {
    store.set({ showVelocities: velocityToggle.input.checked });
  });

  runToggle.input.addEventListener("change", () => {
    store.set({ running: runToggle.input.checked });
  });

  branchesSlider.addEventListener("input", () => {
    store.set({
      branchesPerSeed: clamp(
        Math.round(Number(branchesSlider.value)),
        MIN_BRANCHES,
        MAX_BRANCHES,
      ),
    });
  });

  clearButton.addEventListener("click", () => {
    onClearFaults();
  });

  // Reflect store changes (including programmatic ones) back into the panel.
  // Writing input.value/checked does not fire an "input"/"change" event, so this
  // cannot loop back into store.set.
  store.subscribe((changed) => {
    const state = store.getAll();
    if (changed.has("level")) {
      levelSlider.value = String(state.level);
      renderLevelLabel();
    }
    if (changed.has("seaLevel")) {
      seaSlider.value = String(state.seaLevel);
      renderSeaLabel();
    }
    if (changed.has("autoRotate")) {
      rotateToggle.input.checked = state.autoRotate;
    }
    if (changed.has("projection")) {
      projectionSelect.value = state.projection;
    }
    if (changed.has("viewMode")) {
      viewSelect.value = state.viewMode;
    }
    if (changed.has("seedMode")) {
      seedToggle.input.checked = state.seedMode;
    }
    if (changed.has("showVelocities")) {
      velocityToggle.input.checked = state.showVelocities;
    }
    if (changed.has("branchesPerSeed")) {
      branchesSlider.value = String(state.branchesPerSeed);
      renderBranchesLabel();
    }
    if (changed.has("running")) {
      runToggle.input.checked = state.running;
    }
  });

  panel.append(
    title,
    levelLabel,
    levelSlider,
    seaLabel,
    seaSlider,
    rotateToggle.row,
    projectionLabel,
    projectionSelect,
    viewLabel,
    viewSelect,
    seedToggle.row,
    velocityToggle.row,
    branchesLabel,
    branchesSlider,
    runToggle.row,
    clearButton,
    readout,
    massEl,
    note,
  );
  parent.appendChild(panel);

  return {
    setCellCount: (count: number) => {
      cellEl.textContent = `Cells: ${count.toLocaleString()}`;
    },
    setPlateCount: (count: number) => {
      plateEl.textContent = `Plates: ${count.toLocaleString()}`;
    },
    setFps: (fps: number) => {
      fpsEl.textContent = `FPS: ${fps.toFixed(0)}`;
    },
    setMassReadout: (liveMass: number, drift: number) => {
      const driftPct = (drift * 100).toFixed(3);
      const sign = drift >= 0 ? "+" : "";
      massEl.textContent = `Crust mass: ${liveMass.toFixed(2)} (drift ${sign}${driftPct}%)`;
    },
    setNote: (text: string) => {
      note.textContent = text;
    },
  };
};

interface SelectOption {
  value: string;
  label: string;
}

const makeSelect = (
  options: SelectOption[],
  selected: string,
): HTMLSelectElement => {
  const select = document.createElement("select");
  Object.assign(select.style, {
    width: "100%",
    marginBottom: "10px",
    padding: "4px 6px",
    background: "rgba(20, 26, 38, 0.9)",
    color: "#e6ecf5",
    border: "1px solid rgba(120, 140, 180, 0.3)",
    borderRadius: "6px",
    font: "13px/1.4 ui-sans-serif, system-ui, sans-serif",
    cursor: "pointer",
  } satisfies Partial<CSSStyleDeclaration>);
  for (const option of options) {
    const element = document.createElement("option");
    element.value = option.value;
    element.textContent = option.label;
    if (option.value === selected) element.selected = true;
    select.append(element);
  }
  return select;
};

const makeButton = (labelText: string): HTMLButtonElement => {
  const button = document.createElement("button");
  button.textContent = labelText;
  Object.assign(button.style, {
    width: "100%",
    marginBottom: "10px",
    padding: "6px 8px",
    background: "rgba(79, 143, 240, 0.18)",
    color: "#e6ecf5",
    border: "1px solid rgba(120, 140, 180, 0.35)",
    borderRadius: "6px",
    font: "13px/1.4 ui-sans-serif, system-ui, sans-serif",
    cursor: "pointer",
  } satisfies Partial<CSSStyleDeclaration>);
  return button;
};

interface Toggle {
  row: HTMLLabelElement;
  input: HTMLInputElement;
}

const makeToggle = (labelText: string, checked: boolean): Toggle => {
  const row = document.createElement("label");
  Object.assign(row.style, {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: "10px",
    marginBottom: "8px",
    cursor: "pointer",
    opacity: "0.95",
  } satisfies Partial<CSSStyleDeclaration>);

  const text = document.createElement("span");
  text.textContent = labelText;

  const input = document.createElement("input");
  input.type = "checkbox";
  input.checked = checked;
  Object.assign(input.style, {
    width: "32px",
    height: "18px",
    cursor: "pointer",
    accentColor: "#4f8ff0",
  } satisfies Partial<CSSStyleDeclaration>);

  row.append(text, input);
  return { row, input };
};

const makeLabel = (): HTMLLabelElement => {
  const label = document.createElement("label");
  Object.assign(label.style, {
    display: "block",
    marginBottom: "4px",
    opacity: "0.85",
  } satisfies Partial<CSSStyleDeclaration>);
  return label;
};

const makeSlider = (
  min: number,
  max: number,
  stepValue: number,
  value: number,
): HTMLInputElement => {
  const slider = document.createElement("input");
  slider.type = "range";
  slider.min = String(min);
  slider.max = String(max);
  slider.step = String(stepValue);
  slider.value = String(value);
  Object.assign(slider.style, {
    width: "100%",
    marginBottom: "10px",
    accentColor: "#4f8ff0",
  } satisfies Partial<CSSStyleDeclaration>);
  return slider;
};
