# Tectonic World-Builder — Prototype Design Doc

> Hand-off document for Cursor. Scope is deliberately tight: build **Prototype 1** (render a sphere of discrete cells coloured by elevation) on a foundation that lets plates and tectonics drop in later **without restructuring**. The "Out of Scope (Later Phases)" section exists so the prototype's architecture anticipates what's coming — read it before designing interfaces, but **do not build it**.

---

## 1. Concept

A realistic world-building program/game. A planet is a 3D sphere covered in cells:

- **Fine cells** hold local geology (at minimum, elevation; later velocity, density, crust type, age).
- **Plates** are _groups of fine cells sharing a plate ID_ — not a separate mesh.

The player "cracks the lid" of a freshly-cooled planet to define the initial plates, then observes and optionally directs tectonics over hundreds of millions of years until they get a configuration they like.

**Realism stance:** the simulation should be as geologically plausible as possible _on its own_, but the player must be able to override any value (elevation, velocity, plate assignment) in arbitrary, physically-unrealistic ways. Sim-driven and player-driven writes go through the **same** data path — manipulation is a first-class feature, not a hack.

---

## 2. Tech Decisions (settled)

| Decision                | Choice                                                            | Rationale                                                                                                                                                                                               |
| ----------------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Language / platform     | **TypeScript in the browser**                                     | Dev background is web; the language is not the bottleneck for 1k–40k cells. Render path and algorithms are. Keep sim logic pure + data-oriented so it _could_ later move to WASM/GPU without a rewrite. |
| Rendering               | **Three.js + Vite**                                               | GPU draws the cells; JS only touches data when it changes.                                                                                                                                              |
| Cell tiling (prototype) | **Icosphere → Goldberg dual**                                     | Even cell sizes, clean neighbour graph, mostly hexagons + exactly 12 pentagons (topological necessity).                                                                                                 |
| Fundamental cell unit   | **Hex/pentagon faces** (the dual), not triangle vertices          | Matches the visual + data model the user wants.                                                                                                                                                         |
| Cell count              | **Adjustable via subdivision level**; start ~level 5 (~10k cells) | "As many as perform well." Each level ≈ ×4 cells. Render is not the limiter; later simulation is.                                                                                                       |
| Data layout             | **Data-oriented: flat typed arrays indexed by cell**              | Near-native iteration, zero GC pressure, trivial serialisation, port-friendly.                                                                                                                          |
| Future tiling           | **Spherical Voronoi (Lloyd-relaxed)**                             | Organic look. Must be swappable later — hence the `CellMesh` interface below.                                                                                                                           |

---

## 3. The Core Architecture (build this carefully)

### 3.1 The `CellMesh` interface — the single most important decision

The rest of the system **never knows what shape the cells are**. It only ever sees a cell graph behind an interface. `IcosphereMesh` implements it now; `VoronoiMesh` implements it later; the swap is a constructor change.

```ts
export interface CellMesh {
    readonly cellCount: number;
    neighbors(i: number): readonly number[]; // adjacency, precomputed once at build
    position(i: number): Vec3; // cell centre on the unit sphere
    polygon(i: number): readonly Vec3[]; // corner vertices, for rendering the face
    area(i: number): number; // cell area (for later conservation maths)
}
```

**Rule:** plates, simulation, and rendering consume _only_ this interface. No downstream code may reference icosphere-specific internals.

### 3.2 Data-oriented cell storage

A "cell" is just an index `i`. The mesh (or a parallel `CellData` store) owns flat typed arrays:

```ts
elevation: Float32Array; // length = cellCount; prototype scale e.g. -10..+10
plateId: Int32Array; // present in prototype, all 0 / unused until plates phase
// later: velocity (Float32Array x3), density, crustType, age, ...
```

No `Cell` class with methods. This is the structure to keep even if hot loops later move to WASM/GPU.

### 3.3 Icosphere → dual construction

1. Subdivide an icosahedron `N` times → triangle mesh. The triangle **vertices** become **cell centres**.
2. Build the **dual**: each cell is the polygon connecting the centroids of the triangles around a vertex → the Goldberg hex/pentagon tiling.
3. Neighbour graph falls straight out of triangle adjacency — precompute once, store as flat arrays.
4. `N` is the subdivision-level parameter driving cell count.

### 3.4 Render path

- Build **one** mesh where each cell is a triangle fan from its centre to its corners.
- Cell colour = flat function of `elevation` (vertex colours, flat-shaded → visibly discrete facets; this is the point of the prototype — to _see_ the structure is right).
- Updating elevation = rewriting a colour buffer. Cheap. The static geometry never rebuilds.

### 3.5 Decoupling that matters later (cheap to honour now)

- **Simulation and rendering run at different rates.** Tectonics steps slowly (≤ ~1 Hz or per user tick); rendering runs at 60fps just re-colouring a static mesh. Keep the step function separate from the render loop even in the prototype.
- Keep the simulation step a **pure function over typed arrays** so it can later move into a **Web Worker** (and the UI never stutters) without redesign.

---

## 4. Prototype 1 — Scope & Deliverable

**Goal:** prove "sphere + cells render correctly" on the real foundation.

**Must deliver:**

- A rotatable sphere of **visibly discrete** icosphere cells (hex faces + 12 pentagons).
- Each cell flat-coloured by an `elevation` value held in a `Float32Array`.
- **Adjustable subdivision level** (cell-count dial), starting ~level 5.
- An **FPS + cell-count readout**.
- Built behind the `CellMesh` interface with typed-array storage.

**Suggested project shape:**

```
src/
  mesh/
    CellMesh.ts          // interface + Vec3 type
    IcosphereMesh.ts     // subdivision + dual + neighbour graph
  data/
    CellData.ts          // typed arrays (elevation, plateId)
  render/
    CellRenderer.ts      // builds Three.js geometry, updates colour buffer
  sim/
    step.ts              // empty/no-op stub now; pure fn (mesh, data) => void
  main.ts                // Three.js scene, camera, orbit controls, UI readout
  ui/
    controls.ts          // subdivision slider, FPS/cell-count display
```

**Acceptance:**

- Changing subdivision level rebuilds the sphere at the new cell count.
- A test pattern of elevation values (e.g. noise or banding) shows distinct per-cell colours.
- No downstream file imports icosphere internals — only `CellMesh`.
- Steady 60fps at level 5 on a typical laptop GPU.

**Explicitly NOT in Prototype 1:** plates, plate cracking, movement, real tectonic rules, Voronoi, workers. Stubs only where noted.

---

## 5. Out of Scope (Later Phases) — context, do not build

Brief notes so the prototype's interfaces don't paint us into a corner.

- **Cracking the lid (plate definition).** Seed N points, assign each fine cell to nearest seed (Voronoi-on-sphere or flood-fill) → write `plateId`. Plates are just cell groups; no second mesh. Player picks seeds interactively.
- **Plate properties.** Per-plate velocity (a tangent vector / rotation about an Euler pole), density, etc. Likely a small `Plate[]` keyed by `plateId`, alongside the per-cell arrays.
- **Plates move, but the mesh does NOT.** _The_ conceptual trap. Cells are fixed on the sphere. A plate "moving" = **advecting** crust data (elevation, type, age) from cell to cell in the direction of motion, and reassigning `plateId` at boundaries. Not "move the mesh." Design data flow to make per-cell reassignment + data transfer natural.
- **Spherical geometry maths.** Velocities are tangent vectors; directions follow great circles; motion is **rotation (quaternions / Euler poles)**, not 2D translation. Budget real time here.
- **Boundary classification.** Convergent / divergent / transform emerge from _relative velocity across a plate edge_. Elevation responds: mountains, trenches, ridges. This is the heart of the sim and the hardest tuning problem.
- **Conservation / mass budget.** Crust created at rifts, destroyed at subduction. Track area/mass or the world inflates or vanishes over deep time. (`area(i)` on `CellMesh` exists for this.)
- **Numerical drift.** Hundreds of millions of years = many steps; small errors compound. Watch stability.
- **Web Worker for simulation.** Move the pure step function off the main thread so the UI never stutters. Architecture already allows this if `step.ts` stays pure over typed arrays.
- **Voronoi upgrade.** `VoronoiMesh implements CellMesh` (Lloyd-relaxed seed points). One-time build cost at startup; everything downstream unchanged.
- **Performance ceiling.** If pushed to ~100k+ cells with heavy per-step physics, options are WASM for hot loops or **WebGPU compute shaders** — both reachable from the browser, so the high-performance path never forces leaving the web.

---

## 6. Known Risks / "Unknown Unknowns" (summary)

1. **Moving plates = advection, not mesh translation** — easiest wall to hit.
2. **Spherical maths** (tangent vectors, great circles, Euler-pole rotation) is more involved than flat-grid intuition.
3. **Conservation** of crust over deep time.
4. **Boundary behaviour tuning** — the real difficulty of believable terrain.
5. **Long-run numerical drift.**

None of these affect Prototype 1's code, but all are why the `CellMesh` interface, typed-array storage, `area()`, and a pure simulation step are mandated now.
