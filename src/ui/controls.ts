import { clamp } from "../math/scalar";
import type { Projection } from "../render/projection";
import { resetTuning, setTuning, type Tuning, tuning } from "../sim/tuning";
import type { SettingsStore, ViewMode } from "./settingsStore";

/** Imperative handle for pushing live sim readouts back into the panels. */
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

/** One tunable simulation constant exposed as a slider. */
interface TuningSliderSpec {
  key: keyof Tuning;
  label: string;
  min: number;
  max: number;
  step: number;
}

/** A collapsible group of related tuning sliders. */
interface TuningGroupSpec {
  title: string;
  sliders: TuningSliderSpec[];
}

/**
 * Every tuning constant, grouped by the part of the simulation it governs. The
 * ranges are generous bands around the defaults so the whole feel can be dialled
 * without breaking the maths; values clamp to these bounds.
 */
const TUNING_GROUPS: TuningGroupSpec[] = [
  {
    title: "Relief / elevation",
    sliders: [
      {
        key: "ridgeElevation",
        label: "Ridge crest",
        min: -8,
        max: 0,
        step: 0.1,
      },
      {
        key: "abyssElevation",
        label: "Abyssal floor",
        min: -10,
        max: 0,
        step: 0.1,
      },
      {
        key: "continentBase",
        label: "Continent base",
        min: 0,
        max: 5,
        step: 0.1,
      },
      {
        key: "continentThicknessRef",
        label: "Continent thickness ref",
        min: 0,
        max: 30,
        step: 0.5,
      },
      {
        key: "continentThicknessGain",
        label: "Continent thickness gain",
        min: 0,
        max: 1,
        step: 0.01,
      },
      {
        key: "collisionUplift",
        label: "Collision uplift",
        min: 0,
        max: 10,
        step: 0.1,
      },
      { key: "arcUplift", label: "Arc uplift", min: 0, max: 10, step: 0.1 },
      {
        key: "trenchDeepen",
        label: "Trench deepen",
        min: 0,
        max: 10,
        step: 0.1,
      },
      { key: "ridgePull", label: "Ridge pull", min: 0, max: 1, step: 0.01 },
      {
        key: "relaxRate",
        label: "Isostatic relax rate",
        min: 0,
        max: 0.5,
        step: 0.005,
      },
      {
        key: "elevationMin",
        label: "Elevation clamp min",
        min: -20,
        max: 0,
        step: 0.5,
      },
      {
        key: "elevationMax",
        label: "Elevation clamp max",
        min: 0,
        max: 20,
        step: 0.5,
      },
    ],
  },
  {
    title: "Plate forces / motion",
    sliders: [
      {
        key: "densityRef",
        label: "Density reference",
        min: 2.5,
        max: 3.2,
        step: 0.01,
      },
      {
        key: "intrinsicWeight",
        label: "Intrinsic drive",
        min: 0,
        max: 5,
        step: 0.1,
      },
      { key: "slabWeight", label: "Slab pull", min: 0, max: 5, step: 0.1 },
      { key: "ridgeWeight", label: "Ridge push", min: 0, max: 5, step: 0.1 },
      {
        key: "collisionWeight",
        label: "Collision resistance",
        min: 0,
        max: 1,
        step: 0.05,
      },
      {
        key: "collisionLockGain",
        label: "Collision lock gain",
        min: 1,
        max: 40,
        step: 1,
      },
      {
        key: "sutureClosingFraction",
        label: "Suture closing threshold",
        min: 0,
        max: 1,
        step: 0.01,
      },
      {
        key: "sutureCoupling",
        label: "Suture coupling /Myr",
        min: 0,
        max: 0.5,
        step: 0.01,
      },
      {
        key: "sutureMergeTime",
        label: "Suture merge time (Myr)",
        min: 0,
        max: 400,
        step: 10,
      },
      {
        key: "sutureMaxMergedArea",
        label: "Max merged plate area",
        min: 0.05,
        max: 1,
        step: 0.05,
      },
      { key: "speedGain", label: "Speed gain", min: 0, max: 0.2, step: 0.005 },
      { key: "maxSpeed", label: "Max speed", min: 0, max: 0.1, step: 0.001 },
      { key: "motionRelax", label: "Motion relax", min: 0, max: 1, step: 0.01 },
    ],
  },
  {
    title: "Crust & density",
    sliders: [
      {
        key: "maxOceanicAge",
        label: "Max oceanic age",
        min: 10,
        max: 500,
        step: 10,
      },
      {
        key: "continentalDensity",
        label: "Continental density",
        min: 2.0,
        max: 3.2,
        step: 0.01,
      },
      {
        key: "oceanicDensityYoung",
        label: "Oceanic density (young)",
        min: 2.5,
        max: 3.2,
        step: 0.01,
      },
      {
        key: "oceanicDensityOld",
        label: "Oceanic density (old)",
        min: 2.5,
        max: 3.5,
        step: 0.01,
      },
      {
        key: "oceanicThickness",
        label: "Oceanic thickness",
        min: 1,
        max: 30,
        step: 0.5,
      },
      {
        key: "continentalThicknessThreshold",
        label: "Continental threshold",
        min: 5,
        max: 50,
        step: 0.5,
      },
      {
        key: "maxCrustThickness",
        label: "Max crust thickness",
        min: 10,
        max: 120,
        step: 1,
      },
      {
        key: "delaminationRate",
        label: "Delamination /Myr",
        min: 0,
        max: 2,
        step: 0.05,
      },
      {
        key: "crustSpreadStart",
        label: "Crust spreads above",
        min: 8,
        max: 60,
        step: 0.5,
      },
      {
        key: "crustSpreadRate",
        label: "Crust spread rate /Myr",
        min: 0,
        max: 0.3,
        step: 0.005,
      },
    ],
  },
  {
    title: "Advection",
    sliders: [
      {
        key: "accretionFraction",
        label: "Accretion fraction",
        min: 0,
        max: 1,
        step: 0.01,
      },
      {
        key: "subductionErosion",
        label: "Subduction erosion",
        min: 0,
        max: 0.5,
        step: 0.01,
      },
      {
        key: "maxThickeningPerMyr",
        label: "Max thickening /Myr",
        min: 0.1,
        max: 5,
        step: 0.1,
      },
      {
        key: "riftThicknessFloor",
        label: "Rift thickness floor",
        min: 0,
        max: 5,
        step: 0.1,
      },
      {
        key: "riftDensity",
        label: "Rift density",
        min: 2.5,
        max: 3.2,
        step: 0.01,
      },
    ],
  },
  {
    title: "Boundaries & timing",
    sliders: [
      { key: "dtMyr", label: "Myr per tick", min: 0.5, max: 10, step: 0.5 },
      {
        key: "activeSpeedEps",
        label: "Active speed threshold",
        min: 0,
        max: 0.0005,
        step: 0.00001,
      },
    ],
  },
];

/**
 * Framework-free overlay split into two panels. The left "Simulation" panel owns
 * world structure (subdivision, branches, seeds, run state) plus every tunable
 * simulation constant as live sliders; the right "View" panel owns what the user
 * sees (projection, view mode, overlays, sea level). The settings store remains
 * the single source of truth for view/structure settings; tuning sliders write
 * straight into the live `tuning` singleton and call `onTuningChange` so the
 * scene can recolour immediately. Clear-faults stays a dedicated action.
 */
export const createControls = (
  parent: HTMLElement,
  store: SettingsStore,
  onClearFaults: () => void,
  onTuningChange: () => void,
): Controls => {
  const initial = store.getAll();

  // --- Simulation panel (top-left) -----------------------------------------
  const sim = makePanel("Simulation", "left");

  const levelLabel = makeLabel();
  const levelSlider = makeSlider(MIN_LEVEL, MAX_LEVEL, 1, initial.level);

  const branchesLabel = makeLabel();
  const branchesSlider = makeSlider(
    MIN_BRANCHES,
    MAX_BRANCHES,
    1,
    initial.branchesPerSeed,
  );

  const seedToggle = makeToggle(
    "Place plate seeds (click globe)",
    initial.seedMode,
  );
  const runToggle = makeToggle("Run simulation", initial.running);
  const clearButton = makeButton("Clear faults");

  const readout = document.createElement("div");
  Object.assign(readout.style, {
    display: "flex",
    justifyContent: "space-between",
    gap: "12px",
    fontVariantNumeric: "tabular-nums",
    opacity: "0.95",
  } satisfies Partial<CSSStyleDeclaration>);
  const cellEl = document.createElement("span");
  const plateEl = document.createElement("span");
  readout.append(cellEl, plateEl);

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

  // Tuning sliders, grouped and collapsible. Each remembers how to redraw its
  // own label so a "reset to defaults" can refresh them in place.
  const tuningHeading = document.createElement("div");
  tuningHeading.textContent = "Tuning";
  Object.assign(tuningHeading.style, {
    fontWeight: "600",
    margin: "12px 0 6px",
    letterSpacing: "0.02em",
  } satisfies Partial<CSSStyleDeclaration>);

  interface SliderRef {
    spec: TuningSliderSpec;
    slider: HTMLInputElement;
    render: () => void;
  }
  const sliderRefs: SliderRef[] = [];

  const buildTuningGroup = (group: TuningGroupSpec): HTMLDetailsElement => {
    const details = document.createElement("details");
    Object.assign(details.style, {
      marginBottom: "6px",
      borderTop: "1px solid rgba(120, 140, 180, 0.18)",
      paddingTop: "6px",
    } satisfies Partial<CSSStyleDeclaration>);

    const summary = document.createElement("summary");
    summary.textContent = group.title;
    Object.assign(summary.style, {
      cursor: "pointer",
      opacity: "0.9",
      marginBottom: "6px",
    } satisfies Partial<CSSStyleDeclaration>);
    details.append(summary);

    for (const spec of group.sliders) {
      const label = makeLabel();
      const decimals = decimalsOf(spec.step);
      const slider = makeSlider(
        spec.min,
        spec.max,
        spec.step,
        tuning[spec.key],
      );
      const render = (): void => {
        label.textContent = `${spec.label}: ${tuning[spec.key].toFixed(
          decimals,
        )}`;
      };
      render();
      slider.addEventListener("input", () => {
        const value = clamp(Number(slider.value), spec.min, spec.max);
        setTuning({ [spec.key]: value } as Partial<Tuning>);
        render();
        onTuningChange();
      });
      sliderRefs.push({ spec, slider, render });
      details.append(label, slider);
    }
    return details;
  };

  const tuningGroups = TUNING_GROUPS.map(buildTuningGroup);

  const resetButton = makeButton("Reset tuning to defaults");
  resetButton.addEventListener("click", () => {
    resetTuning();
    for (const { spec, slider, render } of sliderRefs) {
      slider.value = String(tuning[spec.key]);
      render();
    }
    onTuningChange();
  });

  const renderLevelLabel = (): void => {
    levelLabel.textContent = `Subdivision level: ${store.get("level")}`;
  };
  const renderBranchesLabel = (): void => {
    branchesLabel.textContent = `Branches per seed: ${store.get(
      "branchesPerSeed",
    )}`;
  };
  renderLevelLabel();
  renderBranchesLabel();
  cellEl.textContent = "Cells: --";
  plateEl.textContent = "Plates: --";
  massEl.textContent = "Mass: --";

  levelSlider.addEventListener("input", () => {
    store.set({
      level: clamp(Math.round(Number(levelSlider.value)), MIN_LEVEL, MAX_LEVEL),
    });
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
  seedToggle.input.addEventListener("change", () => {
    store.set({ seedMode: seedToggle.input.checked });
  });
  runToggle.input.addEventListener("change", () => {
    store.set({ running: runToggle.input.checked });
  });
  clearButton.addEventListener("click", () => {
    onClearFaults();
  });

  sim.panel.append(
    levelLabel,
    levelSlider,
    branchesLabel,
    branchesSlider,
    seedToggle.row,
    runToggle.row,
    clearButton,
    readout,
    massEl,
    note,
    tuningHeading,
    ...tuningGroups,
    resetButton,
  );

  // --- View panel (top-right) ----------------------------------------------
  const view = makePanel("View", "right");

  const projectionLabel = makeLabel();
  projectionLabel.textContent = "Projection";
  const projectionSelect = makeSelect(
    [
      { value: "sphere", label: "Sphere" },
      { value: "mercator", label: "Mercator map" },
    ],
    initial.projection,
  );

  const viewLabel = makeLabel();
  viewLabel.textContent = "View";
  const viewSelect = makeSelect(
    [
      { value: "colour", label: "Elevation colour" },
      { value: "data", label: "Data (all properties)" },
      { value: "crust", label: "Crust type" },
      { value: "density", label: "Density" },
      { value: "boundary", label: "Boundaries" },
    ],
    initial.viewMode,
  );

  const seaLabel = makeLabel();
  const seaSlider = makeSlider(
    MIN_SEA_LEVEL,
    MAX_SEA_LEVEL,
    0.5,
    initial.seaLevel,
  );
  const renderSeaLabel = (): void => {
    seaLabel.textContent = `Sea level: ${store.get("seaLevel").toFixed(1)}`;
  };
  renderSeaLabel();

  const rotateToggle = makeToggle("Auto-rotate", initial.autoRotate);
  const platesToggle = makeToggle("Show plates overlay", initial.showPlates);
  const velocityToggle = makeToggle(
    "Show plate velocities",
    initial.showVelocities,
  );

  const fpsEl = document.createElement("div");
  Object.assign(fpsEl.style, {
    marginTop: "6px",
    fontVariantNumeric: "tabular-nums",
    opacity: "0.95",
  } satisfies Partial<CSSStyleDeclaration>);
  fpsEl.textContent = "FPS: --";

  projectionSelect.addEventListener("change", () => {
    store.set({ projection: projectionSelect.value as Projection });
  });
  viewSelect.addEventListener("change", () => {
    store.set({ viewMode: viewSelect.value as ViewMode });
  });
  seaSlider.addEventListener("input", () => {
    store.set({
      seaLevel: clamp(Number(seaSlider.value), MIN_SEA_LEVEL, MAX_SEA_LEVEL),
    });
  });
  rotateToggle.input.addEventListener("change", () => {
    store.set({ autoRotate: rotateToggle.input.checked });
  });
  platesToggle.input.addEventListener("change", () => {
    store.set({ showPlates: platesToggle.input.checked });
  });
  velocityToggle.input.addEventListener("change", () => {
    store.set({ showVelocities: velocityToggle.input.checked });
  });

  view.panel.append(
    projectionLabel,
    projectionSelect,
    viewLabel,
    viewSelect,
    seaLabel,
    seaSlider,
    rotateToggle.row,
    platesToggle.row,
    velocityToggle.row,
    fpsEl,
  );

  // Reflect store changes (including programmatic ones) back into the panels.
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
    if (changed.has("showPlates")) {
      platesToggle.input.checked = state.showPlates;
    }
    if (changed.has("branchesPerSeed")) {
      branchesSlider.value = String(state.branchesPerSeed);
      renderBranchesLabel();
    }
    if (changed.has("running")) {
      runToggle.input.checked = state.running;
    }
  });

  parent.append(sim.panel, view.panel);

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

interface Panel {
  panel: HTMLDivElement;
  title: HTMLDivElement;
}

/** A fixed, scrollable glass panel pinned to one side of the viewport. */
const makePanel = (titleText: string, side: "left" | "right"): Panel => {
  const panel = document.createElement("div");
  Object.assign(panel.style, {
    position: "fixed",
    top: "12px",
    padding: "12px 14px",
    background: "rgba(10, 14, 22, 0.78)",
    border: "1px solid rgba(120, 140, 180, 0.25)",
    borderRadius: "10px",
    color: "#e6ecf5",
    font: "13px/1.4 ui-sans-serif, system-ui, sans-serif",
    backdropFilter: "blur(6px)",
    userSelect: "none",
    minWidth: "240px",
    maxWidth: "300px",
    maxHeight: "calc(100vh - 24px)",
    overflowY: "auto",
  } satisfies Partial<CSSStyleDeclaration>);
  panel.style[side] = "12px";

  const title = document.createElement("div");
  title.textContent = titleText;
  Object.assign(title.style, {
    fontWeight: "600",
    marginBottom: "10px",
    letterSpacing: "0.02em",
  } satisfies Partial<CSSStyleDeclaration>);
  panel.append(title);

  return { panel, title };
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

/** Decimal places implied by a slider step, for tidy value labels. */
const decimalsOf = (step: number): number => {
  const s = String(step);
  const dot = s.indexOf(".");
  return dot < 0 ? 0 : s.length - dot - 1;
};
