import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { IcosphereMesh } from "./mesh/IcosphereMesh";
import type { CellMesh } from "./mesh/CellMesh";
import { CellData, seedTestElevation } from "./data/CellData";
import { CellRenderer } from "./render/CellRenderer";
import { step } from "./sim/step";
import { createControls } from "./ui/controls";

const INITIAL_LEVEL = 5;
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
controls.autoRotate = false;
controls.autoRotateSpeed = 0.6;

// --- Active world (rebuilt when the subdivision level changes) -------------

let cellMesh: CellMesh;
let cellData: CellData;
let cellRenderer: CellRenderer | null = null;

const rebuild = (level: number): void => {
    if (cellRenderer) {
        scene.remove(cellRenderer.mesh);
        cellRenderer.dispose();
        cellRenderer = null;
    }

    cellMesh = new IcosphereMesh(level);
    cellData = new CellData(cellMesh.cellCount);
    seedTestElevation(cellMesh, cellData);

    cellRenderer = new CellRenderer(cellMesh);
    cellRenderer.updateColours(cellData);
    scene.add(cellRenderer.mesh);

    ui.setCellCount(cellMesh.cellCount);
};

const ui = createControls(document.body, INITIAL_LEVEL, {
    onLevelChange: level => rebuild(level),
});

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
};
animate();

window.addEventListener("resize", () => {
    camera.aspect = window.innerWidth / window.innerHeight;
    camera.updateProjectionMatrix();
    renderer.setSize(window.innerWidth, window.innerHeight);
});
