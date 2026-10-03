import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { describeBench, parseBenchSpec } from "./bench/benchScenario";
import { Picker } from "./interaction/Picker";
import { ArrayCellMesh } from "./mesh/ArrayCellMesh";
import { CellLabels } from "./render/CellLabels";
import { CellRenderer, type ColourMode } from "./render/CellRenderer";
import { MAP_HALF_EXTENT, type Projection } from "./render/projection";
import { simOptions, tuning } from "./sim/tuning";
import { createControls } from "./ui/controls";
import {
  createSettingsStore,
  type PlateOverlay,
  type ViewMode,
} from "./ui/settingsStore";
import type { Snapshot } from "./worker/protocol";
import { platesFromOmegas, SimClient } from "./worker/SimClient";

const INITIAL_LEVEL = 6;
const INITIAL_SEA_LEVEL = 0;
const INITIAL_AUTO_ROTATE = false;
const INITIAL_PROJECTION: Projection = "sphere";
const INITIAL_VIEW_MODE: ViewMode = "colour";
const INITIAL_SEED_MODE = true;
const INITIAL_BRANCHES_PER_SEED = 3;
const INITIAL_SHOW_VELOCITIES = false;
const INITIAL_PLATE_OVERLAY: PlateOverlay = "colours";
const INITIAL_UI_HIDDEN = false;
const INITIAL_RUNNING = false;
/**
 * Default simulation rate. Tectonics is decoupled from rendering and ticks
 * slowly, but the rate is adjustable: judging the world against Earth means
 * running it for hundreds of Myr, and Earth's own steady state took billions.
 */
const INITIAL_TICKS_PER_SECOND = 2;
/** Pointer travel (px) above which a press counts as an orbit drag, not a click. */
const CLICK_DRAG_THRESHOLD = 6;

/**
 * `?bench=level:8,seeds:12,ticks:200` runs the benchmark scenario in the worker
 * and logs its timings and metric trajectory to the console.
 */
const BENCH = new URLSearchParams(window.location.search).get("bench");

const appElement = document.getElementById("app");
if (!appElement) throw new Error("Missing #app element");

const scene = new THREE.Scene();
scene.background = new THREE.Color(0x05070d);

const camera = new THREE.PerspectiveCamera(
  45,
  window.innerWidth / window.innerHeight,
  0.01,
  100,
);
camera.position.set(0, 1.4, 2.6);

/** Margin around the 2x2 Mercator map so it never touches the viewport edge. */
const MAP_MARGIN = 1.1;

/** Orthographic camera framing the flat Mercator map (looking straight down -z). */
const mapCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 100);
mapCamera.position.set(0, 0, 5);

const setMapFrustum = (aspect: number): void => {
  const halfH = MAP_HALF_EXTENT * MAP_MARGIN;
  const halfW = halfH * aspect;
  mapCamera.left = -halfW;
  mapCamera.right = halfW;
  mapCamera.top = halfH;
  mapCamera.bottom = -halfH;
  mapCamera.updateProjectionMatrix();
};
setMapFrustum(window.innerWidth / window.innerHeight);

const renderer = new THREE.WebGLRenderer({ antialias: true });
renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
renderer.setSize(window.innerWidth, window.innerHeight);
appElement.appendChild(renderer.domElement);

const controls = new OrbitControls(camera, renderer.domElement);
controls.enableDamping = true;
controls.minDistance = 1.2;
controls.maxDistance = 8;
controls.autoRotate = INITIAL_AUTO_ROTATE;
controls.autoRotateSpeed = 0.6;

// Map navigation: left-drag pans, wheel zooms; rotation is disabled so the map
// stays flat. Reuses OrbitControls (what MapControls is built on).
const mapControls = new OrbitControls(mapCamera, renderer.domElement);
mapControls.enableDamping = true;
mapControls.enableRotate = false;
mapControls.screenSpacePanning = true;
mapControls.mouseButtons = {
  LEFT: THREE.MOUSE.PAN,
  MIDDLE: THREE.MOUSE.DOLLY,
  RIGHT: THREE.MOUSE.PAN,
};
mapControls.enabled = false;

/** The camera + controls pair currently driving the view. */
let activeCamera: THREE.Camera = camera;
let activeControls: OrbitControls = controls;

const cellLabels = new CellLabels(document.body);
scene.add(cellLabels.object3D);

const picker = new Picker(camera, renderer.domElement);

// --- Active world -----------------------------------------------------------
// The simulation lives in a worker. The main thread holds the mesh (sent once
// per world) and the latest snapshot, and only ever reads them.

let cellMesh: ArrayCellMesh | null = null;
let cellRenderer: CellRenderer | null = null;
/** Seeds in the current fault network; faults are drawn only once there are some. */
let seedCount = 0;

/** Single source of truth for every UI setting. */
const settings = createSettingsStore({
  level: INITIAL_LEVEL,
  seaLevel: INITIAL_SEA_LEVEL,
  autoRotate: INITIAL_AUTO_ROTATE,
  projection: INITIAL_PROJECTION,
  viewMode: INITIAL_VIEW_MODE,
  seedMode: INITIAL_SEED_MODE,
  branchesPerSeed: INITIAL_BRANCHES_PER_SEED,
  showVelocities: INITIAL_SHOW_VELOCITIES,
  plateOverlay: INITIAL_PLATE_OVERLAY,
  running: INITIAL_RUNNING,
  ticksPerSecond: INITIAL_TICKS_PER_SECOND,
  uiHidden: INITIAL_UI_HIDDEN,
});

/** Map the UI view mode onto the renderer's colour mode. */
const colourMode = (): ColourMode => {
  const viewMode = settings.get("viewMode");
  return viewMode === "colour" ? "elevation" : viewMode;
};

const recolour = (): void => {
  const snapshot = client.snapshot;
  if (!snapshot || !cellRenderer) return;
  cellRenderer.updateColours(
    snapshot.buffers,
    colourMode(),
    settings.get("seaLevel"),
    snapshot.buffers.cellKind,
  );
};

/**
 * Refresh whatever the current plate overlay draws. The coloured fill is only
 * the "colours" mode; the boundary lines are drawn in both "lines" and
 * "colours".
 */
const refreshPlateOverlay = (): void => {
  const snapshot = client.snapshot;
  const overlay = settings.get("plateOverlay");
  if (!snapshot || !cellRenderer || overlay === "off") return;
  if (overlay === "colours") cellRenderer.updatePlateOverlay(snapshot.buffers);
  cellRenderer.updatePlateLines(snapshot.buffers.plateId);
};

const applyVelocities = (): void => {
  cellRenderer?.setVelocitiesVisible(settings.get("showVelocities"));
};

/** Set the plates-overlay visibility and refresh it when it is shown. */
const applyShowPlates = (): void => {
  const overlay = settings.get("plateOverlay");
  refreshPlateOverlay();
  cellRenderer?.setPlatesVisible(overlay === "colours");
  cellRenderer?.setPlateLinesVisible(overlay !== "off");
};

/**
 * A tuning slider moved: forward it to the worker, which reads it on its next
 * tick, and recolour now so colour-ramp edits show immediately even when
 * paused. Tuning stays mirrored here because the colour ramps read it too.
 */
const onTuningChange = (): void => {
  client.setTuning(tuning);
  client.setSimOptions(simOptions);
  recolour();
  refreshPlateOverlay();
};

/**
 * Switch between the sphere and the flat Mercator map: re-project the renderer
 * and labels, swap the active camera + controls, and point the picker at the
 * new camera so seed placement keeps working.
 */
const applyProjection = (): void => {
  const projection = settings.get("projection");
  const onMap = projection === "mercator";
  cellRenderer?.setProjection(projection);
  cellLabels.setProjection(projection);

  activeCamera = onMap ? mapCamera : camera;
  activeControls = onMap ? mapControls : controls;
  controls.enabled = !onMap;
  mapControls.enabled = onMap;
  activeControls.update();

  picker.setProjection(projection, activeCamera);
};

const applyViewMode = (): void => {
  const viewMode = settings.get("viewMode");
  const showLabels = viewMode === "data";
  recolour();
  cellRenderer?.setEdgesVisible(viewMode === "data");
  cellRenderer?.setFaultsVisible(seedCount > 0);
  applyVelocities();
  cellLabels.setVisible(showLabels);
  if (showLabels) {
    ui.setNote(
      cellLabels.isSuppressed
        ? "Labels hidden: too many cells. Lower subdivision level."
        : "Showing all per-cell properties.",
    );
  } else if (settings.get("seedMode")) {
    ui.setNote("Click the globe to drop plate seeds; drag to orbit.");
  } else {
    ui.setNote("");
  }
};

/** A new world's geometry has arrived: rebuild everything drawn from it. */
const onMesh = (arrays: ConstructorParameters<typeof ArrayCellMesh>[0]) => {
  if (cellRenderer) {
    scene.remove(cellRenderer.object3D);
    cellRenderer.dispose();
  }
  cellMesh = new ArrayCellMesh(arrays);
  cellRenderer = new CellRenderer(cellMesh);
  scene.add(cellRenderer.object3D);
  cellRenderer.setProjection(settings.get("projection"));
  cellLabels.build(cellMesh);
  seedCount = 0;
  ui.setCellCount(cellMesh.cellCount);
  applyViewMode();
  applyShowPlates();
};

/** Apply the latest world state to the renderer and panels. */
const onSnapshot = (snapshot: Snapshot): void => {
  if (!cellRenderer) return;
  const start = performance.now();
  const { buffers } = snapshot;
  if (snapshot.faults) {
    cellRenderer.updateFaults(snapshot.faults.cracked);
    cellRenderer.updateSeeds(snapshot.faults.seeds);
    seedCount = snapshot.faults.seeds.length;
    cellRenderer.setFaultsVisible(seedCount > 0);
  }
  recolour();
  refreshPlateOverlay();
  cellRenderer.updateVelocities(
    platesFromOmegas(snapshot.plateOmegas),
    buffers.plateId,
    buffers.arcKind,
  );
  cellLabels.refresh(buffers);
  ui.setPlateCount(snapshot.plateCount);
  ui.setMassReadout(snapshot.mass.liveMass, snapshot.mass.drift);
  if (snapshot.metrics) ui.setMetrics(snapshot.metrics);
  if (snapshot.summaries) ui.setPlates(snapshot.summaries);
  ui.setTimings(snapshot.simMs, performance.now() - start);
};

const client = new SimClient({
  onMesh,
  onSnapshot,
  onBench: (result) => {
    console.log(describeBench(result));
    console.log(JSON.stringify(result));
    (window as unknown as { benchResult: unknown }).benchResult = result;
    // The benchmark ran on default tuning; put the live tuning back.
    onTuningChange();
  },
});

const rebuild = (level: number): void => {
  client.init(level, settings.get("branchesPerSeed"), tuning, simOptions);
};

/** Drop a seed at the picked junction; the worker regrows faults and plates. */
const placeSeed = (event: PointerEvent): void => {
  if (!cellMesh) return;
  const junction = picker.pickJunction(event, cellMesh.boundaryGraph());
  if (junction === null) return;
  client.addSeed(junction);
};

const applyRunning = (): void => {
  if (settings.get("running")) client.run(settings.get("ticksPerSecond"));
  else client.pause();
};

const ui = createControls(
  document.body,
  settings,
  () => client.clearFaults(),
  onTuningChange,
  () => client.step(),
);

// React to setting changes. The store is the source of truth; these subscriptions
// own the side effects each setting triggers.
settings.on("level", (level) => rebuild(level));
settings.on("seaLevel", () => recolour());
settings.on("autoRotate", (enabled) => {
  controls.autoRotate = enabled;
});
settings.on("projection", () => applyProjection());
settings.on("viewMode", () => applyViewMode());
settings.on("seedMode", () => applyViewMode());
settings.on("branchesPerSeed", (branches) => client.setBranches(branches));
settings.on("showVelocities", () => applyVelocities());
settings.on("plateOverlay", () => applyShowPlates());
settings.on("running", () => applyRunning());
settings.on("ticksPerSecond", () => {
  if (settings.get("running")) applyRunning();
});

rebuild(INITIAL_LEVEL);
applyProjection();
applyRunning();
if (BENCH !== null) client.bench(parseBenchSpec(BENCH));

// --- Seed placement: a click (not an orbit drag) in seed mode --------------

let pointerDownX = 0;
let pointerDownY = 0;
renderer.domElement.addEventListener("pointerdown", (event) => {
  pointerDownX = event.clientX;
  pointerDownY = event.clientY;
});
renderer.domElement.addEventListener("pointerup", (event) => {
  if (!settings.get("seedMode")) return;
  const travel = Math.hypot(
    event.clientX - pointerDownX,
    event.clientY - pointerDownY,
  );
  if (travel > CLICK_DRAG_THRESHOLD) return; // was an orbit drag
  placeSeed(event);
});

// --- Render loop: 60fps, only re-colours / redraws a static mesh -----------

let fpsEma = 60;
let lastTime = performance.now();

const animate = (): void => {
  requestAnimationFrame(animate);

  const now = performance.now();
  const delta = now - lastTime;
  lastTime = now;
  if (delta > 0) {
    const instantaneous = 1000 / delta;
    fpsEma += (instantaneous - fpsEma) * 0.1;
    ui.setFps(fpsEma);
  }

  activeControls.update();
  renderer.render(scene, activeCamera);
  cellLabels.update(activeCamera);
  cellLabels.render(scene, activeCamera);
};
animate();

window.addEventListener("resize", () => {
  const aspect = window.innerWidth / window.innerHeight;
  camera.aspect = aspect;
  camera.updateProjectionMatrix();
  setMapFrustum(aspect);
  renderer.setSize(window.innerWidth, window.innerHeight);
  cellLabels.resize();
});
