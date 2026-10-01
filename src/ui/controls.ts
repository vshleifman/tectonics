import { clamp } from "../math/scalar";
import { CM_PER_YR_PER_RAD_MYR, type PlateSummary } from "../plates/PlateData";
import { colourForPlate, toCssRgb } from "../render/colour";
import type { Projection } from "../render/projection";
import {
  AGE_BIN_MYR,
  AGE_BINS,
  EARTH,
  type WorldMetrics,
} from "../sim/metrics";
import {
  resetTuning,
  setSimOptions,
  setTuning,
  simOptions,
  type Tuning,
  tuning,
} from "../sim/tuning";
import type { SettingsStore, ViewMode } from "./settingsStore";

/** Imperative handle for pushing live sim readouts back into the panels. */
export interface Controls {
  setCellCount: (count: number) => void;
  setPlateCount: (count: number) => void;
  setFps: (fps: number) => void;
  /** Worker ms per tick, and main-thread ms to apply each snapshot. */
  setTimings: (simMs: number, uiMs: number) => void;
  setMassReadout: (liveMass: number, drift: number) => void;
  setNote: (text: string) => void;
  /** Replace the plate list with the live set of plates. */
  setPlates: (plates: readonly PlateSummary[]) => void;
  /** Update the diagnostics panel with a fresh reading of the world. */
  setMetrics: (metrics: WorldMetrics) => void;
}

/**
 * The one data hue used in the diagnostics chart — the panel accent, which
 * clears the lightness band, chroma floor and 3:1 contrast against the panel
 * surface. Earth's reference distribution is drawn as a dashed line rather than
 * a second hue: as a categorical pair the two would fail the normal-vision
 * separation floor, and a target is an annotation anyway, not a series.
 */
const CHART_INK = "#4f8ff0";
const CHART_REFERENCE_INK = "rgba(230, 236, 245, 0.45)";

/** Legend for the boundary velocity arrows, matching the renderer's tints. */
const ARROW_LEGEND: ReadonlyArray<{ colour: string; label: string }> = [
  { colour: "rgb(255, 107, 87)", label: "Convergent" },
  { colour: "rgb(107, 184, 255)", label: "Divergent" },
  { colour: "rgb(255, 219, 92)", label: "Transform" },
];

const MIN_LEVEL = 2;
const MAX_LEVEL = 8;
const MIN_SEA_LEVEL = -8;
const MAX_SEA_LEVEL = 8;
const MIN_BRANCHES = 1;
const MAX_BRANCHES = 5;
const MIN_TICK_RATE = 1;
const MAX_TICK_RATE = 30;

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
      { key: "slabWeight", label: "Slab pull", min: 0, max: 5, step: 0.1 },
      {
        key: "slabPullOnsetAge",
        label: "Slab pull onset age",
        min: 0,
        max: 100,
        step: 5,
      },
      {
        key: "slabPullSaturationAge",
        label: "Slab pull saturation age",
        min: 20,
        max: 300,
        step: 5,
      },
      {
        key: "ridgeWeight",
        label: "Ridge push",
        min: 0,
        max: 0.5,
        step: 0.005,
      },
      { key: "mantleDrag", label: "Mantle drag", min: 20, max: 2000, step: 20 },
      {
        key: "continentalDrag",
        label: "Continental keel drag",
        min: 1,
        max: 12,
        step: 0.5,
      },
      {
        key: "collisionDrag",
        label: "Collision resistance",
        min: 0,
        max: 2000,
        step: 20,
      },
      {
        key: "collisionLockGain",
        label: "Collision lock gain",
        min: 1,
        max: 40,
        step: 1,
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
        key: "mergeQuietSpeed",
        label: "Merge quiet speed",
        min: 0,
        max: 0.02,
        step: 0.0005,
      },
    ],
  },
  {
    title: "Rifting",
    sliders: [
      {
        key: "riftStrength",
        label: "Plate strength",
        min: 0,
        max: 3,
        step: 0.05,
      },
      {
        key: "minRiftArea",
        label: "Min riftable area",
        min: 0.005,
        max: 0.2,
        step: 0.005,
      },
      {
        key: "weaknessRelief",
        label: "Weakness relief",
        min: 0,
        max: 1,
        step: 0.05,
      },
      {
        key: "sutureWeakness",
        label: "Suture weakness",
        min: 0,
        max: 1,
        step: 0.05,
      },
      {
        key: "riftMarginWeakness",
        label: "Rift margin weakness",
        min: 0,
        max: 1,
        step: 0.05,
      },
      {
        key: "weaknessDecayTime",
        label: "Weakness memory (Myr)",
        min: 100,
        max: 5000,
        step: 100,
      },
      {
        key: "insulationRate",
        label: "Insulation heating /Myr",
        min: 0,
        max: 0.05,
        step: 0.001,
      },
      {
        key: "insulationCooling",
        label: "Insulation cooling /Myr",
        min: 0,
        max: 0.05,
        step: 0.001,
      },
      {
        key: "insulationWeakening",
        label: "Heat weakening",
        min: 0,
        max: 1,
        step: 0.05,
      },
      {
        key: "insulationDrive",
        label: "Heat-driven extension",
        min: 0,
        max: 5,
        step: 0.1,
      },
      {
        key: "minPlateArea",
        label: "Min fragment area",
        min: 0,
        max: 0.01,
        step: 0.0005,
      },
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
  onStepOnce: () => void,
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
  const rateLabel = makeLabel();
  const rateSlider = makeSlider(
    MIN_TICK_RATE,
    MAX_TICK_RATE,
    1,
    initial.ticksPerSecond,
  );
  const renderRateLabel = (): void => {
    const rate = store.get("ticksPerSecond");
    rateLabel.textContent = `Sim rate: ${rate} ticks/s (${(
      rate * tuning.dtMyr
    ).toFixed(0)} Myr/s)`;
  };
  const stepButton = makeButton("Step once");
  const clearButton = makeButton("Clear faults");
  // A modelling choice rather than a dial: with it off, rifting is driven by
  // inherited weakness and slab tension alone and the mantle-heat field is
  // never touched, which is one fewer pass over every cell each tick.
  const insulationToggle = makeToggle(
    "Supercontinent insulation",
    simOptions.supercontinentInsulation,
  );
  insulationToggle.input.addEventListener("change", () => {
    setSimOptions({
      supercontinentInsulation: insulationToggle.input.checked,
    });
    onTuningChange();
  });

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
        if (spec.key === "dtMyr") renderRateLabel();
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

  const diagnostics = makeDiagnostics();

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
  renderRateLabel();
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
  rateSlider.addEventListener("input", () => {
    store.set({
      ticksPerSecond: clamp(
        Math.round(Number(rateSlider.value)),
        MIN_TICK_RATE,
        MAX_TICK_RATE,
      ),
    });
  });
  stepButton.addEventListener("click", () => {
    onStepOnce();
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
    rateLabel,
    rateSlider,
    stepButton,
    clearButton,
    insulationToggle.row,
    readout,
    massEl,
    note,
    diagnostics.details,
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

  // Colour key for the boundary arrows; only meaningful while they are drawn,
  // so it follows the velocity toggle.
  const arrowLegend = document.createElement("div");
  Object.assign(arrowLegend.style, {
    display: initial.showVelocities ? "flex" : "none",
    gap: "10px",
    flexWrap: "wrap",
    margin: "-2px 0 8px",
    fontSize: "11px",
    opacity: "0.8",
  } satisfies Partial<CSSStyleDeclaration>);
  for (const entry of ARROW_LEGEND) {
    const item = document.createElement("span");
    Object.assign(item.style, {
      display: "flex",
      alignItems: "center",
      gap: "4px",
    } satisfies Partial<CSSStyleDeclaration>);
    const dash = document.createElement("span");
    Object.assign(dash.style, {
      width: "12px",
      height: "2px",
      borderRadius: "1px",
      background: entry.colour,
    } satisfies Partial<CSSStyleDeclaration>);
    const text = document.createElement("span");
    text.textContent = entry.label;
    item.append(dash, text);
    arrowLegend.append(item);
  }

  // Collapsible roster of every live plate: its overlay colour, how fast it is
  // moving and which way. Rebuilt wholesale on each update (there are tens of
  // plates at most, and the <details> wrapper keeps its open/closed state).
  const platesDetails = document.createElement("details");
  platesDetails.open = true;
  Object.assign(platesDetails.style, {
    marginBottom: "8px",
    borderTop: "1px solid rgba(120, 140, 180, 0.18)",
    paddingTop: "6px",
  } satisfies Partial<CSSStyleDeclaration>);
  const platesSummary = document.createElement("summary");
  platesSummary.textContent = "Plates";
  Object.assign(platesSummary.style, {
    cursor: "pointer",
    opacity: "0.9",
    marginBottom: "6px",
  } satisfies Partial<CSSStyleDeclaration>);
  const platesList = document.createElement("div");
  Object.assign(platesList.style, {
    maxHeight: "240px",
    overflowY: "auto",
    paddingRight: "2px",
  } satisfies Partial<CSSStyleDeclaration>);
  platesDetails.append(platesSummary, platesList);

  const fpsEl = document.createElement("div");
  Object.assign(fpsEl.style, {
    marginTop: "6px",
    fontVariantNumeric: "tabular-nums",
    opacity: "0.95",
  } satisfies Partial<CSSStyleDeclaration>);
  fpsEl.textContent = "FPS: --";

  const timingEl = document.createElement("div");
  Object.assign(timingEl.style, {
    fontVariantNumeric: "tabular-nums",
    opacity: "0.75",
    fontSize: "11px",
  } satisfies Partial<CSSStyleDeclaration>);
  timingEl.textContent = "Sim: -- ms/tick · UI: -- ms";

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
    arrowLegend,
    platesDetails,
    fpsEl,
    timingEl,
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
      arrowLegend.style.display = state.showVelocities ? "flex" : "none";
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
    if (changed.has("ticksPerSecond")) {
      rateSlider.value = String(state.ticksPerSecond);
      renderRateLabel();
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
    setTimings: (simMs: number, uiMs: number) => {
      timingEl.textContent = `Sim: ${simMs.toFixed(1)} ms/tick · UI: ${uiMs.toFixed(1)} ms`;
    },
    setMassReadout: (liveMass: number, drift: number) => {
      const driftPct = (drift * 100).toFixed(3);
      const sign = drift >= 0 ? "+" : "";
      massEl.textContent = `Crust mass: ${liveMass.toFixed(2)} (drift ${sign}${driftPct}%)`;
    },
    setNote: (text: string) => {
      note.textContent = text;
    },
    setMetrics: diagnostics.update,
    setPlates: (plates: readonly PlateSummary[]) => {
      platesSummary.textContent = `Plates (${plates.length})`;
      platesList.replaceChildren(
        ...(plates.length > 0
          ? plates.map(makePlateRow)
          : [makeEmptyPlateNote()]),
      );
    },
  };
};

/** The diagnostics section: a set of readings, and the age-area chart. */
interface Diagnostics {
  details: HTMLDetailsElement;
  update: (metrics: WorldMetrics) => void;
}

/**
 * Earth's sea-floor age-area distribution, as a share per 20 Myr bin.
 *
 * Ocean floor is created at ridges at a roughly steady rate and consumed at
 * trenches at a rate proportional to how much of it there is, which gives an
 * age-area distribution that falls off linearly to zero at the oldest surviving
 * crust (~180 Myr). Derived here from that straight line rather than typed in,
 * so it stays tied to {@link EARTH.maxOceanAge}.
 */
const earthAgeHistogram = (bins: number): number[] => {
  const weights: number[] = [];
  let total = 0;
  for (let i = 0; i < bins; i++) {
    const centre = (i + 0.5) * AGE_BIN_MYR;
    const w = Math.max(0, 1 - centre / EARTH.maxOceanAge);
    weights.push(w);
    total += w;
  }
  return weights.map((w) => (total > 0 ? w / total : 0));
};

const SVG_NS = "http://www.w3.org/2000/svg";
const CHART_HEIGHT = 52;
const CHART_WIDTH = 240;
/** Gap (in chart units) between neighbouring bars, so fills never touch. */
const BAR_GAP = 2;

/**
 * The diagnostics section: how Earth-like the world currently is.
 *
 * Every reading is shown beside the real Earth value it is trying to reach, so
 * a change to the physics can be judged instead of admired. The chart is the
 * sharpest of them: sea-floor age-area, against the straight line Earth's falls
 * along. A world that subducts too little grows an old tail; one that subducts
 * everything young collapses toward the first bin.
 */
const makeDiagnostics = (): Diagnostics => {
  const details = document.createElement("details");
  details.open = true;
  Object.assign(details.style, {
    marginTop: "10px",
    marginBottom: "6px",
    borderTop: "1px solid rgba(120, 140, 180, 0.18)",
    paddingTop: "6px",
  } satisfies Partial<CSSStyleDeclaration>);

  const summary = document.createElement("summary");
  summary.textContent = "Diagnostics";
  Object.assign(summary.style, {
    cursor: "pointer",
    opacity: "0.9",
    marginBottom: "8px",
  } satisfies Partial<CSSStyleDeclaration>);

  // Readings: label, this world, Earth. Three columns so the comparison reads
  // across rather than needing the numbers held in mind.
  const grid = document.createElement("div");
  Object.assign(grid.style, {
    display: "grid",
    gridTemplateColumns: "1fr auto auto",
    columnGap: "10px",
    rowGap: "3px",
    fontVariantNumeric: "tabular-nums",
    marginBottom: "10px",
  } satisfies Partial<CSSStyleDeclaration>);

  const heading = (text: string, align: string): HTMLDivElement => {
    const el = document.createElement("div");
    el.textContent = text;
    Object.assign(el.style, {
      opacity: "0.5",
      fontSize: "11px",
      textAlign: align,
    } satisfies Partial<CSSStyleDeclaration>);
    return el;
  };
  grid.append(
    heading("", "left"),
    heading("world", "right"),
    heading("Earth", "right"),
  );

  const addRow = (label: string, earth: string): HTMLDivElement => {
    const name = document.createElement("div");
    name.textContent = label;
    name.style.opacity = "0.8";
    const value = document.createElement("div");
    value.style.textAlign = "right";
    value.textContent = "--";
    const target = document.createElement("div");
    target.textContent = earth;
    Object.assign(target.style, {
      textAlign: "right",
      opacity: "0.45",
    } satisfies Partial<CSSStyleDeclaration>);
    grid.append(name, value, target);
    return value;
  };

  const elapsedValue = addRow("Elapsed", "--");
  const plateValue = addRow(
    "Plates (major)",
    `${EARTH.totalPlates} (${EARTH.majorPlates})`,
  );
  const continentValue = addRow(
    "Continental",
    `${Math.round(EARTH.continentalArea * 100)}%`,
  );
  const ageValue = addRow(
    "Ocean age mean/max",
    `${EARTH.meanOceanAge}/${EARTH.maxOceanAge}`,
  );
  const recycleValue = addRow(
    "Ocean recycling",
    `${EARTH.oceanRecyclingMyr} Myr`,
  );
  const boundaryValue = addRow("Conv/div/trans %", "");
  const driveValue = addRow(
    "Speed vs trench",
    `+${EARTH.speedTrenchCorrelation.toFixed(1)}`,
  );

  // --- Sea-floor age-area chart -------------------------------------------
  const chartTitle = document.createElement("div");
  chartTitle.textContent = "Sea floor by age (share of ocean)";
  Object.assign(chartTitle.style, {
    opacity: "0.6",
    fontSize: "11px",
    marginBottom: "4px",
  } satisfies Partial<CSSStyleDeclaration>);

  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", `0 0 ${CHART_WIDTH} ${CHART_HEIGHT}`);
  svg.setAttribute("preserveAspectRatio", "none");
  Object.assign(svg.style, {
    display: "block",
    width: "100%",
    height: `${CHART_HEIGHT}px`,
    overflow: "visible",
  } satisfies Partial<CSSStyleDeclaration>);

  const reference = earthAgeHistogram(AGE_BINS);
  const barWidth = CHART_WIDTH / AGE_BINS;

  const bars: SVGRectElement[] = [];
  const labels: SVGTitleElement[] = [];
  for (let i = 0; i < AGE_BINS; i++) {
    const rect = document.createElementNS(SVG_NS, "rect");
    rect.setAttribute("x", String(i * barWidth + BAR_GAP / 2));
    rect.setAttribute("width", String(Math.max(1, barWidth - BAR_GAP)));
    rect.setAttribute("rx", "2");
    rect.setAttribute("fill", CHART_INK);
    const title = document.createElementNS(SVG_NS, "title");
    rect.append(title);
    svg.append(rect);
    bars.push(rect);
    labels.push(title);
  }

  // Earth's distribution, over the bars: an annotation, so it is dashed and in
  // muted ink rather than competing as a second series.
  const referenceLine = document.createElementNS(SVG_NS, "polyline");
  referenceLine.setAttribute("fill", "none");
  referenceLine.setAttribute("stroke", CHART_REFERENCE_INK);
  referenceLine.setAttribute("stroke-width", "1.5");
  referenceLine.setAttribute("stroke-dasharray", "4 3");
  referenceLine.setAttribute("vector-effect", "non-scaling-stroke");
  const referenceTitle = document.createElementNS(SVG_NS, "title");
  referenceTitle.textContent = "Earth: linear falloff to ~180 Myr";
  referenceLine.append(referenceTitle);
  svg.append(referenceLine);

  const axis = document.createElement("div");
  Object.assign(axis.style, {
    display: "flex",
    justifyContent: "space-between",
    opacity: "0.45",
    fontSize: "10px",
    marginTop: "2px",
  } satisfies Partial<CSSStyleDeclaration>);
  const youngEnd = document.createElement("span");
  youngEnd.textContent = "0";
  const oldEnd = document.createElement("span");
  oldEnd.textContent = `${AGE_BIN_MYR * AGE_BINS}+ Myr`;
  const key = document.createElement("span");
  key.textContent = "- - Earth";
  key.style.opacity = "0.8";
  axis.append(youngEnd, key, oldEnd);

  details.append(summary, grid, chartTitle, svg, axis);

  const update = (metrics: WorldMetrics): void => {
    elapsedValue.textContent = `${Math.round(metrics.elapsedMyr)} Myr`;
    plateValue.textContent = `${metrics.plateCount} (${metrics.majorPlateCount})`;
    continentValue.textContent = `${(metrics.continentalArea * 100).toFixed(1)}%`;
    ageValue.textContent = `${Math.round(metrics.meanOceanAge)}/${Math.round(
      metrics.maxOceanAge,
    )}`;
    recycleValue.textContent =
      metrics.oceanRecyclingMyr === null
        ? "never"
        : `${Math.round(metrics.oceanRecyclingMyr)} Myr`;
    const [conv, div, trans] = metrics.boundaryShare;
    boundaryValue.textContent = `${Math.round(conv * 100)}/${Math.round(
      div * 100,
    )}/${Math.round(trans * 100)}`;
    driveValue.textContent =
      metrics.speedTrenchCorrelation === null
        ? "--"
        : formatSigned(metrics.speedTrenchCorrelation);

    // One scale for both, so the bars can be read against the reference; a
    // little headroom keeps the tallest bar off the top edge.
    const peak = Math.max(...metrics.ageHistogram, ...reference, 0.01);
    const scale = CHART_HEIGHT / (peak * 1.1);

    for (let i = 0; i < AGE_BINS; i++) {
      const share = metrics.ageHistogram[i] ?? 0;
      const height = share * scale;
      bars[i].setAttribute("y", String(CHART_HEIGHT - height));
      bars[i].setAttribute("height", String(Math.max(0, height)));
      const from = i * AGE_BIN_MYR;
      const to = from + AGE_BIN_MYR;
      labels[i].textContent = `${from}-${to} Myr: ${(share * 100).toFixed(
        1,
      )}% of ocean floor`;
    }
    referenceLine.setAttribute(
      "points",
      reference
        .map(
          (share, i) =>
            `${i * barWidth + barWidth / 2},${CHART_HEIGHT - share * scale}`,
        )
        .join(" "),
    );
  };

  return { details, update };
};

/** A correlation with its sign always shown, so -0.1 and +0.1 do not read alike. */
const formatSigned = (value: number): string =>
  `${value >= 0 ? "+" : ""}${value.toFixed(2)}`;

/** Placeholder shown while the world has no plates to list. */
const makeEmptyPlateNote = (): HTMLDivElement => {
  const empty = document.createElement("div");
  empty.textContent = "No plates yet.";
  Object.assign(empty.style, {
    opacity: "0.6",
    fontSize: "11px",
  } satisfies Partial<CSSStyleDeclaration>);
  return empty;
};

/**
 * One plate row: a swatch in the plate's overlay colour, its stable id (the same
 * id the data view labels cells with), an arrow glyph rotated to its heading,
 * and its speed, bearing and share of the globe underneath.
 *
 * Speed is converted out of the simulation's rad/Myr into the cm/yr plate
 * motions are normally quoted in, so the numbers can be sanity-checked against
 * the real Earth (a few cm/yr).
 */
const makePlateRow = (plate: PlateSummary): HTMLDivElement => {
  const row = document.createElement("div");
  Object.assign(row.style, {
    padding: "4px 0",
    borderBottom: "1px solid rgba(120, 140, 180, 0.12)",
  } satisfies Partial<CSSStyleDeclaration>);

  const head = document.createElement("div");
  Object.assign(head.style, {
    display: "flex",
    alignItems: "center",
    gap: "7px",
  } satisfies Partial<CSSStyleDeclaration>);

  const swatch = document.createElement("span");
  Object.assign(swatch.style, {
    width: "11px",
    height: "11px",
    borderRadius: "3px",
    flex: "0 0 auto",
    background: toCssRgb(colourForPlate(plate.id)),
    border: "1px solid rgba(0, 0, 0, 0.35)",
  } satisfies Partial<CSSStyleDeclaration>);

  const name = document.createElement("span");
  name.textContent = `Plate ${plate.id}`;
  Object.assign(name.style, {
    flex: "1 1 auto",
  } satisfies Partial<CSSStyleDeclaration>);

  // The glyph points north at 0 deg and rotates clockwise, which is exactly how
  // the bearing is measured -- so the rotation is the bearing, unmodified.
  const heading = document.createElement("span");
  heading.textContent = plate.heading === null ? "·" : "↑";
  Object.assign(heading.style, {
    flex: "0 0 auto",
    opacity: plate.heading === null ? "0.4" : "0.95",
    transform:
      plate.heading === null
        ? "none"
        : `rotate(${plate.heading.toFixed(1)}deg)`,
  } satisfies Partial<CSSStyleDeclaration>);

  head.append(swatch, name, heading);

  const detail = document.createElement("div");
  const share = `${(plate.areaShare * 100).toFixed(1)}% of globe`;
  detail.textContent =
    plate.heading === null
      ? `still · ${share}`
      : `${(plate.speed * CM_PER_YR_PER_RAD_MYR).toFixed(1)} cm/yr · ` +
        `${plate.heading.toFixed(0).padStart(3, "0")}° · ${share}`;
  Object.assign(detail.style, {
    marginLeft: "18px",
    fontSize: "11px",
    opacity: "0.65",
    fontVariantNumeric: "tabular-nums",
  } satisfies Partial<CSSStyleDeclaration>);

  row.append(head, detail);
  return row;
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
