import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { describeBench, parseBenchSpec } from "./bench/benchScenario";
import { Picker } from "./interaction/Picker";
import { ArrayCellMesh } from "./mesh/ArrayCellMesh";
import { CM_PER_YR_PER_RAD_MYR } from "./plates/PlateData";
import { CellReadout } from "./render/CellReadout";
import { CellRenderer, type ColourMode } from "./render/CellRenderer";
import type { ElevationStyle } from "./render/colour";
import {
    longitudeOf,
    MAP_HALF_EXTENT,
    MAP_HALF_HEIGHT,
    type Projection,
    projectPoint,
} from "./render/projection";
import { simOptions, tuning } from "./sim/tuning";
import { createControls } from "./ui/controls";
import { loadSettings, loadTuning, persistSettings } from "./ui/persist";
import {
    createSettingsStore,
    type HandTool,
    type PlateOverlay,
    type ViewMode,
} from "./ui/settingsStore";
import type { Snapshot } from "./worker/protocol";
import { platesFromOmegas, SimClient } from "./worker/SimClient";

const INITIAL_LEVEL = 6;
const INITIAL_AUTO_ROTATE = false;
const INITIAL_PROJECTION: Projection = "sphere";
const INITIAL_VIEW_MODE: ViewMode = "colour";
const INITIAL_ELEVATION_STYLE: ElevationStyle = "absolute";
const INITIAL_SMOOTH_SURFACE = true;
const INITIAL_HILLSHADE = true;
const INITIAL_HILLSHADE_STRENGTH = 0.8;
const INITIAL_HILLSHADE_EXAGGERATION = 15;
const INITIAL_SEED_MODE = true;
const INITIAL_BRANCHES_PER_SEED = 3;
const INITIAL_SHOW_VELOCITIES = false;
const INITIAL_CELL_READOUT = false;
const INITIAL_PLATE_OVERLAY: PlateOverlay = "colours";
const INITIAL_HAND_TOOL: HandTool = "off";
/** Fast against today's plates (1 to 10 cm/yr), so the hand shows in a few ticks. */
const INITIAL_HAND_SPEED = 15;
const INITIAL_HAND_RADIUS = 20;
/** Metres a second at the brush's centre: a few km of relief in a few seconds. */
const INITIAL_SCULPT_RATE = 200;
const INITIAL_SCULPT_NOISE = 50;
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

/**
 * The map's rectangle, as planes keeping what lies inside it. Cells on the
 * antimeridian and the overlays drawn over them overhang the left and right
 * edges; the renderer clips them off while the map is shown.
 */
const MAP_CLIP_PLANES = [
    new THREE.Plane(new THREE.Vector3(1, 0, 0), MAP_HALF_EXTENT),
    new THREE.Plane(new THREE.Vector3(-1, 0, 0), MAP_HALF_EXTENT),
    new THREE.Plane(new THREE.Vector3(0, 1, 0), MAP_HALF_HEIGHT),
    new THREE.Plane(new THREE.Vector3(0, -1, 0), MAP_HALF_HEIGHT),
];

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

const cellReadout = new CellReadout(document.body);

const picker = new Picker(camera, renderer.domElement);

// --- The hand's outline: a black band round what it reaches, and within it
// one round its core that fills while it is held, until it grips. Bands, not
// lines, since WebGL draws lines a single pixel wide.

/** Segments round each band. */
const HAND_RING_POINTS = 96;
/** Above the plate lines and below the velocity arrows. */
const HAND_RING_LIFT = 0.01;
/** Width of each band (radians of arc): a few pixels at the starting zoom. */
const HAND_RING_WIDTH = 0.008;
/**
 * Share of the hand's reach that is its core, where a gripping hand folds
 * plates together (sim-rs `hand::CORE_SHARE`).
 */
const HAND_CORE_SHARE = 0.5;
/** Running time (ms) the hand must be held in one place before it grips. */
const HAND_GRIP_MS = 5_000;

const makeHandBand = () => {
    // An inner and an outer point at each step round, the last step closing it.
    const positions = new Float32Array((HAND_RING_POINTS + 1) * 2 * 3);
    const index: number[] = [];
    for (let k = 0; k < HAND_RING_POINTS; k++) {
        const i = k * 2;
        index.push(i, i + 1, i + 2, i + 1, i + 3, i + 2);
    }
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
    geometry.setIndex(index);
    const material = new THREE.MeshBasicMaterial({
        color: 0x000000,
        transparent: true,
        depthWrite: false,
        side: THREE.DoubleSide,
    });
    const mesh = new THREE.Mesh(geometry, material);
    mesh.frustumCulled = false;
    mesh.renderOrder = 3;
    mesh.visible = false;
    scene.add(mesh);
    return { positions, geometry, material, mesh };
};
const handRing = makeHandBand();
const handCore = makeHandBand();

/** Where the hand is, or would be put down: the globe under the pointer. */
let handCentre: [number, number, number] | null = null;
/** Whether the hand is held down on the globe. */
let handHeld = false;
/** Where the hand was when its grip began to build. */
let handAnchor: [number, number, number] | null = null;
/** Running time (ms) the hand has been held near {@link handAnchor}. */
let handHeldMs = 0;
/** Whether the hand has been held long enough to grip. */
let handGripping = false;
/** The noise pattern of the current sculpting press. */
let sculptSeed = 0;

/** Whether `tool` shapes the land rather than moving plates. */
const sculpting = (tool: HandTool): boolean =>
    tool === "elevate" || tool === "lower";

/** Lay a band `radius` radians round `c` into `out`, inner and outer edges. */
const traceBand = (
    out: Float32Array,
    c: [number, number, number],
    radius: number,
): void => {
    // Two directions square to the centre and each other span its circle.
    const a: [number, number, number] =
        Math.abs(c[1]) < 0.9 ? [0, 1, 0] : [1, 0, 0];
    const u = new THREE.Vector3(...a)
        .cross(new THREE.Vector3(...c))
        .normalize();
    const v = new THREE.Vector3(...c).cross(u);
    const projection = settings.get("projection");
    const refLon = longitudeOf(c[0], c[1], c[2]);
    for (let k = 0; k <= HAND_RING_POINTS; k++) {
        const phi = (k / HAND_RING_POINTS) * 2 * Math.PI;
        for (let edge = 0; edge < 2; edge++) {
            const r = radius + (edge - 0.5) * HAND_RING_WIDTH;
            const along = Math.cos(r);
            const s = Math.sin(phi) * Math.sin(r);
            const t = Math.cos(phi) * Math.sin(r);
            const p = projectPoint(
                c[0] * along + u.x * t + v.x * s,
                c[1] * along + u.y * t + v.y * s,
                c[2] * along + u.z * t + v.z * s,
                projection,
                HAND_RING_LIFT,
                refLon,
            );
            out.set(p, (k * 2 + edge) * 3);
        }
    }
};

/**
 * Draw the hand's reach around {@link handCentre}, and while it is held, how
 * near it is to gripping; or hide both.
 */
const drawHandRing = (): void => {
    const c = handCentre;
    if (settings.get("handTool") === "off" || !c) {
        handRing.mesh.visible = false;
        handCore.mesh.visible = false;
        return;
    }
    const radius = (settings.get("handRadius") * Math.PI) / 180;
    traceBand(handRing.positions, c, radius);
    handRing.geometry.attributes.position.needsUpdate = true;
    handRing.material.opacity = handHeld ? 0.9 : 0.5;
    handRing.mesh.visible = true;

    const progress = handGripping ? 1 : Math.min(1, handHeldMs / HAND_GRIP_MS);
    if (!handHeld || progress <= 0) {
        handCore.mesh.visible = false;
        return;
    }
    traceBand(handCore.positions, c, radius * HAND_CORE_SHARE);
    handCore.geometry.attributes.position.needsUpdate = true;
    handCore.geometry.setDrawRange(
        0,
        Math.ceil(progress * HAND_RING_POINTS) * 6,
    );
    handCore.material.opacity = handGripping ? 0.9 : 0.6;
    handCore.mesh.visible = true;
};

// --- Active world -----------------------------------------------------------
// The simulation lives in a worker. The main thread holds the mesh (sent once
// per world) and the latest snapshot, and only ever reads them.

let cellMesh: ArrayCellMesh | null = null;
let cellRenderer: CellRenderer | null = null;
/** Seeds in the current fault network; faults are drawn only once there are some. */
let seedCount = 0;
/**
 * The fault lines and yellow seed markers are a placement aid, drawn on the
 * mesh junctions where the seeds were dropped. Plate motion carries the
 * boundaries off those junctions, so the aid is taken down once the world
 * ticks and put back only when a new network is placed while paused.
 */
let showSeedOverlay = true;
/**
 * Whether the world has ticked. Seeds carve plates out of the mesh as it was
 * built, which a world whose plates have moved no longer matches, so seeding
 * stops until a new world.
 */
let seedingLocked = false;

// What the user last chose, laid over the defaults below. The tuning goes in
// before the control panel is built, which reads it for the sliders.
loadTuning();

/** Single source of truth for every UI setting. */
const settings = createSettingsStore(
    loadSettings({
        level: INITIAL_LEVEL,
        autoRotate: INITIAL_AUTO_ROTATE,
        projection: INITIAL_PROJECTION,
        viewMode: INITIAL_VIEW_MODE,
        resource: 0,
        elevationStyle: INITIAL_ELEVATION_STYLE,
        smoothSurface: INITIAL_SMOOTH_SURFACE,
        hillshade: INITIAL_HILLSHADE,
        hillshadeStrength: INITIAL_HILLSHADE_STRENGTH,
        hillshadeExaggeration: INITIAL_HILLSHADE_EXAGGERATION,
        seedMode: INITIAL_SEED_MODE,
        branchesPerSeed: INITIAL_BRANCHES_PER_SEED,
        showVelocities: INITIAL_SHOW_VELOCITIES,
        cellReadout: INITIAL_CELL_READOUT,
        plateOverlay: INITIAL_PLATE_OVERLAY,
        handTool: INITIAL_HAND_TOOL,
        handSpeed: INITIAL_HAND_SPEED,
        handRadius: INITIAL_HAND_RADIUS,
        sculptRate: INITIAL_SCULPT_RATE,
        sculptNoise: INITIAL_SCULPT_NOISE,
        running: INITIAL_RUNNING,
        ticksPerSecond: INITIAL_TICKS_PER_SECOND,
        uiHidden: INITIAL_UI_HIDDEN,
    }),
);
const persistence = persistSettings(settings);

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
        snapshot.buffers.cellKind,
        settings.get("elevationStyle"),
        {
            hillshade: settings.get("hillshade"),
            strength: settings.get("hillshadeStrength"),
            exaggeration: settings.get("hillshadeExaggeration"),
        },
        settings.get("resource"),
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
    if (overlay === "colours")
        cellRenderer.updatePlateOverlay(snapshot.buffers);
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
    persistence.tuningChanged();
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
    // The hand holds the controls it was put down under.
    if (handHeld) releaseHand();
    const projection = settings.get("projection");
    const onMap = projection === "mercator";
    cellRenderer?.setProjection(projection);

    renderer.clippingPlanes = onMap ? MAP_CLIP_PLANES : [];
    activeCamera = onMap ? mapCamera : camera;
    activeControls = onMap ? mapControls : controls;
    controls.enabled = !onMap;
    mapControls.enabled = onMap;
    activeControls.update();

    picker.setProjection(projection, activeCamera);
    drawHandRing();
};

const applyViewMode = (): void => {
    recolour();
    applySeedOverlay();
    applyVelocities();
    if (settings.get("seedMode") && !seedingLocked) {
        ui.setNote("Click the globe to drop plate seeds; drag to orbit.");
    } else {
        ui.setNote("");
    }
};

/** A new world's geometry has arrived: rebuild everything drawn from it. */
const onMesh = (
    arrays: ConstructorParameters<typeof ArrayCellMesh>[0],
    _generation: number,
    engine: string,
) => {
    if (cellRenderer) {
        scene.remove(cellRenderer.object3D);
        cellRenderer.dispose();
    }
    cellMesh = new ArrayCellMesh(arrays);
    cellRenderer = new CellRenderer(cellMesh);
    cellRenderer.setSmooth(settings.get("smoothSurface"));
    scene.add(cellRenderer.object3D);
    cellRenderer.setProjection(settings.get("projection"));
    seedCount = 0;
    showSeedOverlay = true;
    ui.setCellCount(cellMesh.cellCount);
    ui.setEngine(engine);
    applyViewMode();
    applyShowPlates();
};

/** Apply the latest world state to the renderer and panels. */
const onSnapshot = (snapshot: Snapshot): void => {
    if (!cellRenderer) return;
    const start = performance.now();
    const { buffers } = snapshot;
    const ticked = snapshot.tick > 0;
    if (ticked !== seedingLocked) {
        seedingLocked = ticked;
        ui.setSeedingLocked(ticked);
        applyViewMode();
    }
    if (snapshot.faults) {
        cellRenderer.updateFaults(snapshot.faults.cracked);
        cellRenderer.updateSeeds(snapshot.faults.seeds);
        seedCount = snapshot.faults.seeds.length;
        if (settings.get("running")) showSeedOverlay = false;
        applySeedOverlay();
    }
    recolour();
    refreshPlateOverlay();
    cellRenderer.updateVelocities(
        platesFromOmegas(snapshot.plateOmegas),
        buffers.plateId,
        buffers.arcKind,
    );
    hoverStale = true;
    ui.setPlateCount(snapshot.plateCount);
    ui.setMassReadout(snapshot.mass.liveMass, snapshot.mass.drift);
    if (snapshot.metrics) ui.setMetrics(snapshot.metrics);
    if (snapshot.summaries) ui.setPlates(snapshot.summaries);
    ui.setTimings(snapshot.simMs, performance.now() - start);
};

/**
 * Whether the world being built is a saved one that has just been carried on.
 * Its level is then the saved world's, which the level setting must take
 * without that counting as a request to build a new world.
 */
let applyingResume = false;

const onResumed = (level: number, tick: number): void => {
    applyingResume = true;
    settings.set({ level });
    applyingResume = false;
    // The seed aid sits on the junctions the seeds were dropped on, which a
    // world that has ticked has carried its boundaries away from.
    if (tick > 0) {
        showSeedOverlay = false;
        applySeedOverlay();
    }
};

const client = new SimClient({
    onMesh,
    onResumed,
    onSnapshot,
    onBench: result => {
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

/** Show the seed markers and the fault lines they grew, or hide both. */
const applySeedOverlay = (): void => {
    cellRenderer?.setFaultsVisible(showSeedOverlay && seedCount > 0);
};

/**
 * Take down the seed-placement aid. The lines and markers are fixed to the
 * junctions the seeds were dropped on, and a running world leaves them behind.
 */
const dismissSeedOverlay = (): void => {
    if (!showSeedOverlay) return;
    showSeedOverlay = false;
    applySeedOverlay();
};

/**
 * Run the world while it is set running, or while a plate tool is held on it:
 * the hand moves plates only as the world ticks, so holding it on a paused
 * world runs the world until it is let go. Sculpting needs no time to pass.
 */
const applyRunning = (): void => {
    const pushing = handHeld && !sculpting(settings.get("handTool"));
    if (settings.get("running") || pushing) {
        dismissSeedOverlay();
        client.run(settings.get("ticksPerSecond"));
    } else {
        client.pause();
    }
};

const ui = createControls(
    document.body,
    settings,
    () => {
        // Clearing the network is a fresh placement, so the aid comes back unless
        // the world is already running.
        showSeedOverlay = !settings.get("running");
        client.clearFaults();
    },
    onTuningChange,
    () => {
        dismissSeedOverlay();
        client.step();
    },
    () => rebuild(settings.get("level")),
);

// React to setting changes. The store is the source of truth; these subscriptions
// own the side effects each setting triggers.
settings.on("level", level => {
    if (!applyingResume) rebuild(level);
});
settings.on("elevationStyle", () => recolour());
settings.on("resource", () => recolour());
settings.on("smoothSurface", smooth => {
    cellRenderer?.setSmooth(smooth);
    recolour();
});
settings.on("hillshade", () => recolour());
settings.on("hillshadeStrength", () => recolour());
settings.on("hillshadeExaggeration", () => recolour());
settings.on("autoRotate", enabled => {
    controls.autoRotate = enabled;
});
settings.on("projection", () => applyProjection());
settings.on("viewMode", () => applyViewMode());
settings.on("seedMode", () => applyViewMode());
settings.on("branchesPerSeed", branches => client.setBranches(branches));
settings.on("showVelocities", () => applyVelocities());
settings.on("plateOverlay", () => applyShowPlates());
settings.on("running", () => applyRunning());
settings.on("ticksPerSecond", () => {
    if (settings.get("running")) applyRunning();
});

// Carry on from the saved world if there is one; otherwise build a new one.
client.init(
    settings.get("level"),
    settings.get("branchesPerSeed"),
    tuning,
    simOptions,
    true,
);
applyProjection();
applyRunning();
if (BENCH !== null) client.bench(parseBenchSpec(BENCH));

// A reload or a closed tab: write out what is pending, and ask the worker to
// do the same. The worker's save may not finish in time, which is why it also
// saves on its own while the world changes.
const saveEverything = (): void => {
    persistence.flush();
    client.save();
};
window.addEventListener("pagehide", saveEverything);
document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") saveEverything();
});

// --- The hand: press and hold with a hand tool picked ----------------------
// Plates around the pointer are drawn toward it or pushed away from it while
// the button is held, on top of their own forces; the worker applies it from
// its next tick. Orbiting is paused meanwhile, so dragging moves the hand.
// A paused world runs while the hand is held, and pauses again when it is let
// go. Held in one place for HAND_GRIP_MS it grips: drawing plates
// in, it folds together those meeting in its core; pushing them apart, it
// rifts the plate under it. Moving it out of its core starts the count again.
// The terrain tools instead raise or lower the land under the brush, at once
// and whether or not the world is running, with one noise pattern per press.

/** Put the hand down at {@link handCentre}, or move it there. */
const sendHand = (): void => {
    const tool = settings.get("handTool");
    if (!handHeld || !handCentre || tool === "off") return;
    const radius = (settings.get("handRadius") * Math.PI) / 180;
    if (sculpting(tool)) {
        const rate = settings.get("sculptRate") / 1000;
        client.setSculpt(
            handCentre,
            radius,
            tool === "elevate" ? rate : -rate,
            settings.get("sculptNoise") / 100,
            sculptSeed,
            tool === "elevate",
        );
        return;
    }
    const speed = settings.get("handSpeed") / CM_PER_YR_PER_RAD_MYR;
    client.setHand(
        handCentre,
        radius,
        tool === "attract" ? speed : -speed,
        handGripping,
    );
};

/** Start the count to a grip afresh, from where the hand is now. */
const resetGrip = (): void => {
    handAnchor = handCentre;
    handHeldMs = 0;
    handGripping = false;
};

/** Count time held towards a grip; called once a frame. */
const holdHand = (deltaMs: number): void => {
    if (!handHeld || handGripping || sculpting(settings.get("handTool"))) {
        return;
    }
    handHeldMs += deltaMs;
    if (handHeldMs >= HAND_GRIP_MS) {
        handGripping = true;
        sendHand();
    }
    drawHandRing();
};

const releaseHand = (): void => {
    if (!handHeld) return;
    handHeld = false;
    resetGrip();
    activeControls.enabled = true;
    // Either may be held: the tool can change while the button is down.
    client.clearHand();
    client.clearSculpt();
    applyRunning();
    drawHandRing();
};

// Capture, so the orbit controls, listening on the canvas itself, see the
// press only after they have been switched off for it.
renderer.domElement.addEventListener(
    "pointerdown",
    event => {
        if (settings.get("handTool") === "off" || event.button !== 0) return;
        const centre = picker.direction(event.clientX, event.clientY);
        if (!centre || !cellMesh) return;
        handCentre = centre;
        handHeld = true;
        resetGrip();
        sculptSeed = Math.floor(Math.random() * 0x1_0000_0000);
        activeControls.enabled = false;
        renderer.domElement.setPointerCapture(event.pointerId);
        sendHand();
        applyRunning();
        drawHandRing();
    },
    { capture: true },
);
renderer.domElement.addEventListener("pointermove", event => {
    if (settings.get("handTool") === "off") return;
    // Off the globe the hand stays where it last was.
    const centre = picker.direction(event.clientX, event.clientY);
    if (centre) handCentre = centre;
    else if (!handHeld) handCentre = null;
    if (handHeld && handCentre && handAnchor) {
        const [a, b] = [handCentre, handAnchor];
        const apart = Math.acos(
            Math.min(1, a[0] * b[0] + a[1] * b[1] + a[2] * b[2]),
        );
        const core =
            (settings.get("handRadius") * Math.PI * HAND_CORE_SHARE) / 180;
        if (apart > core) resetGrip();
    }
    sendHand();
    drawHandRing();
});
renderer.domElement.addEventListener("pointerup", releaseHand);
renderer.domElement.addEventListener("pointercancel", releaseHand);
renderer.domElement.addEventListener("pointerleave", () => {
    if (handHeld) return;
    handCentre = null;
    drawHandRing();
});
window.addEventListener("blur", releaseHand);

settings.on("handTool", () => {
    releaseHand();
    drawHandRing();
});
settings.on("handSpeed", () => sendHand());
settings.on("sculptRate", () => sendHand());
settings.on("sculptNoise", () => sendHand());
settings.on("handRadius", () => {
    sendHand();
    drawHandRing();
});

// --- Seed placement: a click (not an orbit drag) in seed mode --------------

let pointerDownX = 0;
let pointerDownY = 0;
renderer.domElement.addEventListener("pointerdown", event => {
    pointerDownX = event.clientX;
    pointerDownY = event.clientY;
});
renderer.domElement.addEventListener("pointerup", event => {
    if (!settings.get("seedMode") || seedingLocked) return;
    if (settings.get("handTool") !== "off") return;
    const travel = Math.hypot(
        event.clientX - pointerDownX,
        event.clientY - pointerDownY,
    );
    if (travel > CLICK_DRAG_THRESHOLD) return; // was an orbit drag
    placeSeed(event);
});

// --- Cell readout: every property of the cell under the pointer, while it is
// switched on. The pointer and the camera both move the cell under the
// pointer, so it is looked up once a frame while the pointer is over the world.

/** Pointer position over the canvas, or null while it is elsewhere. */
let hoverPoint: { x: number; y: number } | null = null;
/** Cell the readout shows, or -1 for none; also where the next lookup starts. */
let hoveredCell = -1;
/** The readout needs redrawing even if the cell under the pointer is the same. */
let hoverStale = false;

renderer.domElement.addEventListener("pointermove", event => {
    hoverPoint = { x: event.clientX, y: event.clientY };
    hoverStale = true;
});
renderer.domElement.addEventListener("pointerleave", () => {
    hoverPoint = null;
    hoverStale = true;
});

const updateHover = (): void => {
    const snapshot = client.snapshot;
    const point = hoverPoint;
    const cell =
        settings.get("cellReadout") && point && cellMesh && snapshot
            ? picker.pickCell(
                  point.x,
                  point.y,
                  cellMesh,
                  Math.max(0, hoveredCell),
              )
            : null;
    if (cell === null || !point || !snapshot) {
        if (hoveredCell >= 0) cellReadout.hide();
        hoveredCell = -1;
        return;
    }
    if (cell === hoveredCell && !hoverStale) return;
    hoveredCell = cell;
    hoverStale = false;
    cellReadout.show(snapshot.buffers, cell, point.x, point.y);
};

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

    holdHand(delta);
    activeControls.update();
    renderer.render(scene, activeCamera);
    updateHover();
};
animate();

window.addEventListener("resize", () => {
    const aspect = window.innerWidth / window.innerHeight;
    camera.aspect = aspect;
    camera.updateProjectionMatrix();
    setMapFrustum(aspect);
    renderer.setSize(window.innerWidth, window.innerHeight);
});
