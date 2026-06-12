import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { IcosphereMesh } from "./mesh/IcosphereMesh";
import type { CellMesh } from "./mesh/CellMesh";
import { CellData, seedTestElevation } from "./data/CellData";
import { CellRenderer } from "./render/CellRenderer";
import { CellLabels } from "./render/CellLabels";
import { step } from "./sim/step";
import { createControls, type ViewMode } from "./ui/controls";

const INITIAL_LEVEL = 5;
const INITIAL_SEA_LEVEL = 0;
const INITIAL_AUTO_ROTATE = false;
const INITIAL_VIEW_MODE: ViewMode = "colour";
const SIM_INTERVAL_MS = 1000; // tectonics ticks slowly, decoupled from render

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

const cellLabels = new CellLabels(document.body);
scene.add(cellLabels.object3D);

// --- Active world (rebuilt when the subdivision level changes) -------------

let cellMesh: CellMesh;
let cellData: CellData;
let cellRenderer: CellRenderer | null = null;
let seaLevel = INITIAL_SEA_LEVEL;
let viewMode: ViewMode = INITIAL_VIEW_MODE;

const recolour = (): void => {
    cellRenderer?.updateColours(cellData, seaLevel, viewMode === "data");
};

const applyViewMode = (): void => {
    const showLabels = viewMode === "data";
    recolour();
    cellRenderer?.setEdgesVisible(viewMode === "data");
    cellLabels.setVisible(showLabels);
    if (!showLabels) {
        ui.setNote("");
    } else if (cellLabels.isSuppressed) {
        ui.setNote("Labels hidden: too many cells. Lower subdivision level.");
    } else {
        ui.setNote("Showing cell id and elevation.");
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
    seedTestElevation(cellMesh, cellData);

    cellRenderer = new CellRenderer(cellMesh);
    scene.add(cellRenderer.object3D);

    cellLabels.build(cellMesh, cellData);

    ui.setCellCount(cellMesh.cellCount);
    applyViewMode();
};

const ui = createControls(
    document.body,
    {
        level: INITIAL_LEVEL,
        seaLevel: INITIAL_SEA_LEVEL,
        autoRotate: INITIAL_AUTO_ROTATE,
        viewMode: INITIAL_VIEW_MODE,
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
        onViewModeChange: mode => {
            viewMode = mode;
            applyViewMode();
        },
    },
);

rebuild(INITIAL_LEVEL);

// --- Simulation tick: decoupled from rendering -----------------------------
// Pure no-op for now; recolour only if data changed (it never does yet).

window.setInterval(() => {
    step(cellMesh, cellData);
    // When tectonics writes elevation later, recolour here:
    // cellRenderer?.updateColours(cellData);
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

    controls.update();
    renderer.render(scene, camera);
    cellLabels.update(camera);
    cellLabels.render(scene, camera);
};
animate();

window.addEventListener("resize", () => {
    camera.aspect = window.innerWidth / window.innerHeight;
    camera.updateProjectionMatrix();
    renderer.setSize(window.innerWidth, window.innerHeight);
    cellLabels.resize();
});
