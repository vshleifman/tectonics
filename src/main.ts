import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { CellData, seedUniformElevation } from "./data/CellData";
import { Picker } from "./interaction/Picker";
import type { CellMesh } from "./mesh/CellMesh";
import { IcosphereMesh } from "./mesh/IcosphereMesh";
import {
    initialiseCrust,
    PlateData,
    reconcilePlateProperties,
} from "./plates/PlateData";
import { PlateRegistry } from "./plates/PlateRegistry";
import { Plates } from "./plates/Plates";
import { CellLabels } from "./render/CellLabels";
import { CellRenderer, type ColourMode } from "./render/CellRenderer";
import { MAP_HALF_EXTENT, type Projection } from "./render/projection";
import { BoundaryField, classifyBoundaries } from "./sim/boundaries";
import { step } from "./sim/step";
import { createControls, type ViewMode } from "./ui/controls";

const INITIAL_LEVEL = 6;
const INITIAL_SEA_LEVEL = 0;
const INITIAL_AUTO_ROTATE = false;
const INITIAL_PROJECTION: Projection = "sphere";
const INITIAL_VIEW_MODE: ViewMode = "plate";
const INITIAL_SEED_MODE = true;
const INITIAL_BRANCHES_PER_SEED = 3;
const INITIAL_SHOW_VELOCITIES = false;
const INITIAL_RUNNING = false;
const SIM_INTERVAL_MS = 100; // tectonics ticks slowly, decoupled from render
/** Myr of plate motion advanced per simulation tick (tunable). */
const DT_MYR = 2;
/** Pointer travel (px) above which a press counts as an orbit drag, not a click. */
const CLICK_DRAG_THRESHOLD = 6;

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

// --- Active world (rebuilt when the subdivision level changes) -------------

let cellMesh: CellMesh;
let cellData: CellData;
/** Back buffer the simulation gathers into before copying back over `cellData`. */
let backBuffer: CellData;
let cellRenderer: CellRenderer | null = null;
let plates: Plates | null = null;
let plateData: PlateData | null = null;
/** Boundary classification, recomputed whenever plates or crust change. */
let boundaries: BoundaryField | null = null;

/** Persistent plate identity across cracks/merges; reset only on world rebuild. */
const plateRegistry = new PlateRegistry();

/** Scratch buffer for raw flood-fill labels before they are mapped to stable ids. */
let rawLabels = new Int32Array(0);
let seaLevel = INITIAL_SEA_LEVEL;
let projection: Projection = INITIAL_PROJECTION;
let viewMode: ViewMode = INITIAL_VIEW_MODE;
let seedMode = INITIAL_SEED_MODE;
let branchesPerSeed = INITIAL_BRANCHES_PER_SEED;
let showVelocities = INITIAL_SHOW_VELOCITIES;
let running = INITIAL_RUNNING;

/** Map the UI view mode onto the renderer's colour mode. */
const colourMode = (): ColourMode =>
    viewMode === "colour" ? "elevation" : viewMode;

const recolour = (): void => {
    cellRenderer?.updateColours(
        cellData,
        colourMode(),
        seaLevel,
        boundaries?.cellKind,
    );
};

/** Re-derive boundary classification from the current plates and crust. */
const classify = (): void => {
    if (!plateData || !boundaries) return;
    classifyBoundaries(cellMesh, cellData, plateData, boundaries);
};

/**
 * Re-derive plates after a topology change: flood-fill into raw labels, map them
 * onto stable ids (so identity survives splits/merges), reconcile per-plate
 * properties, and refresh the velocity arrows. Per-cell crust is seeded only on
 * `initCrust` (world build); thereafter it persists and advects with cells.
 * Returns the live plate count.
 */
const reassignPlates = (initCrust: boolean): number => {
    if (!plates || !plateData || !cellRenderer) return 0;
    const plateCount = plates.assignPlates(rawLabels);
    const report = plateRegistry.relabel(
        rawLabels,
        plateCount,
        cellData.plateId,
    );
    reconcilePlateProperties(plateData, report);
    if (initCrust) initialiseCrust(cellMesh, cellData, plateData);
    cellRenderer.updateVelocities(plateData, cellData);
    classify();
    return plateCount;
};

const applyVelocities = (): void => {
    cellRenderer?.setVelocitiesVisible(showVelocities);
};

/**
 * Switch between the sphere and the flat Mercator map: re-project the renderer
 * and labels, swap the active camera + controls, and point the picker at the
 * new camera so seed placement keeps working.
 */
const applyProjection = (): void => {
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
    const showLabels = viewMode === "data";
    recolour();
    cellRenderer?.setEdgesVisible(viewMode === "data");
    cellRenderer?.setFaultsVisible((plates?.seeds.length ?? 0) > 0);
    applyVelocities();
    cellLabels.setVisible(showLabels);
    if (showLabels) {
        ui.setNote(
            cellLabels.isSuppressed
                ? "Labels hidden: too many cells. Lower subdivision level."
                : "Showing all per-cell properties.",
        );
    } else if (seedMode) {
        ui.setNote("Click the globe to drop plate seeds; drag to orbit.");
    } else {
        ui.setNote("");
    }
};

const rebuild = (level: number): void => {
    if (cellRenderer) {
        scene.remove(cellRenderer.object3D);
        cellRenderer.dispose();
        cellRenderer = null;
    }

    cellMesh = new IcosphereMesh(level);
    cellData = new CellData(cellMesh.cellCount);
    backBuffer = new CellData(cellMesh.cellCount);
    boundaries = new BoundaryField(
        cellMesh.cellCount,
        cellMesh.boundaryGraph().arcCount,
    );
    seedUniformElevation(cellMesh, cellData);
    rawLabels = new Int32Array(cellMesh.cellCount);

    plates = new Plates(cellMesh);
    plates.setBranchesPerSeed(branchesPerSeed);
    plateRegistry.reset();
    plateData = new PlateData();

    cellRenderer = new CellRenderer(cellMesh);
    scene.add(cellRenderer.object3D);
    const plateCount = reassignPlates(true);
    cellRenderer.updateFaults(plates.cracked);
    cellRenderer.updateSeeds(plates.seeds);

    cellLabels.build(cellMesh, cellData);

    cellRenderer.setProjection(projection);

    ui.setCellCount(cellMesh.cellCount);
    ui.setPlateCount(plateCount);
    applyViewMode();
};

/** Drop a seed at the picked junction, regrow faults, and re-derive plates. */
const placeSeed = (event: PointerEvent): void => {
    if (!plates || !cellRenderer) return;
    const junction = picker.pickJunction(event, cellMesh.boundaryGraph());
    if (junction === null) return;

    plates.addSeed(junction);
    const plateCount = reassignPlates(false);
    cellRenderer.updateFaults(plates.cracked);
    cellRenderer.updateSeeds(plates.seeds);
    cellRenderer.setFaultsVisible(true);
    ui.setPlateCount(plateCount);
    recolour();
    cellLabels.refresh();
};

const ui = createControls(
    document.body,
    {
        level: INITIAL_LEVEL,
        seaLevel: INITIAL_SEA_LEVEL,
        autoRotate: INITIAL_AUTO_ROTATE,
        projection: INITIAL_PROJECTION,
        viewMode: INITIAL_VIEW_MODE,
        seedMode: INITIAL_SEED_MODE,
        branchesPerSeed: INITIAL_BRANCHES_PER_SEED,
        showVelocities: INITIAL_SHOW_VELOCITIES,
        running: INITIAL_RUNNING,
    },
    {
        onLevelChange: level => rebuild(level),
        onSeaLevelChange: nextSeaLevel => {
            seaLevel = nextSeaLevel;
            recolour();
        },
        onAutoRotateChange: enabled => {
            controls.autoRotate = enabled;
        },
        onProjectionChange: nextProjection => {
            projection = nextProjection;
            applyProjection();
        },
        onViewModeChange: mode => {
            viewMode = mode;
            applyViewMode();
        },
        onSeedModeChange: enabled => {
            seedMode = enabled;
            applyViewMode();
        },
        onBranchesChange: branches => {
            branchesPerSeed = branches;
            plates?.setBranchesPerSeed(branches);
        },
        onVelocitiesChange: enabled => {
            showVelocities = enabled;
            applyVelocities();
        },
        onRunChange: enabled => {
            running = enabled;
        },
        onClearFaults: () => {
            if (!plates || !cellRenderer) return;
            plates.clear();
            const plateCount = reassignPlates(false);
            cellRenderer.updateFaults(plates.cracked);
            cellRenderer.updateSeeds(plates.seeds);
            cellRenderer.setFaultsVisible(false);
            ui.setPlateCount(plateCount);
            recolour();
            cellLabels.refresh();
        },
    },
);

rebuild(INITIAL_LEVEL);
applyProjection();

// --- Seed placement: a click (not an orbit drag) in seed mode --------------

let pointerDownX = 0;
let pointerDownY = 0;
renderer.domElement.addEventListener("pointerdown", event => {
    pointerDownX = event.clientX;
    pointerDownY = event.clientY;
});
renderer.domElement.addEventListener("pointerup", event => {
    if (!seedMode) return;
    const travel = Math.hypot(
        event.clientX - pointerDownX,
        event.clientY - pointerDownY,
    );
    if (travel > CLICK_DRAG_THRESHOLD) return; // was an orbit drag
    placeSeed(event);
});

// --- Simulation tick: decoupled from rendering -----------------------------
// Advect crust one step into the back buffer, copy it back over the live data
// (keeping every existing reference to `cellData` valid), then refresh visuals.

const copyCellData = (from: CellData, to: CellData): void => {
    to.elevation.set(from.elevation);
    to.plateId.set(from.plateId);
    to.crustType.set(from.crustType);
    to.age.set(from.age);
    to.density.set(from.density);
};

window.setInterval(() => {
    if (!running || !plateData || !cellRenderer) return;
    step(cellMesh, cellData, backBuffer, plateData, DT_MYR);
    copyCellData(backBuffer, cellData);
    classify();
    recolour();
    cellRenderer.updateVelocities(plateData, cellData);
    cellLabels.refresh();
}, SIM_INTERVAL_MS);

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
