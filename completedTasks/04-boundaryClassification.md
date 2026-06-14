What I implemented
Plate boundaries are now detected and classified as convergent / divergent / transform every step, exposed for downstream sim use and visualised with a debug colour mode. No geology acts on the classification yet (that is tickets 05-07).

Classification (new src/sim/boundaries.ts)

A boundary lives on the dual edge skeleton: each arc in boundaryGraph separates exactly one cell pair (arcCells), so a boundary is simply an arc whose two cells belong to different plates. classifyBoundaries(mesh, data, plateData, out) is a pure function over the mesh interface + flat typed arrays (Worker/WASM-ready, matching step/advect).

For each boundary arc it takes the relative surface velocity vRel = vA - vB at the arc midpoint (reusing PlateData.linearVelocity, i.e. omega x m), decomposes it into the across-boundary normal (cell A -> cell B, projected to the tangent plane via tangent) and the remaining tangential part, then classifies:

- normal dominates and closing (vn > 0) -> convergent
- normal dominates and opening (vn < 0) -> divergent
- tangential dominates -> transform
- relative speed below ACTIVE_SPEED_EPS -> none (interior or inactive boundary)

|vn| >= |vt| splits convergent/divergent from transform at ~45 deg; the threshold and epsilon are module consts.

Representation (resolves the ticket's open question — both, cheaply)

- Canonical per-arc in BoundaryField: arcKind (Int8Array) plus arcClosing (Float32Array, signed normal speed: + closing, - opening). This is the source of truth and what conservative advection / crust lifecycle (05/07) want, and arcClosing is kept so elevation response (06) can scale ridge/trench/mountain magnitude.
- Derived per-cell: cellKind (Uint8Array) + cellIntensity (Float32Array) — each cell takes its strongest incident boundary (max |vn|). This is the cheap form the renderer and 06 read. BoundaryField is allocated once per world and refilled in place, mirroring the CellData back-buffer pattern.

Visualisation — new "Boundaries" cell-colour view mode

- src/render/colour.ts: "boundary" added to ColourMode; colourForCell takes an optional boundaryKind and colourForBoundary maps grey (none) / red (convergent) / blue (divergent) / yellow (transform).
- src/render/CellRenderer.ts: updateColours threads an optional boundaryKind through to the per-cell colour lookup.
- src/ui/controls.ts: "boundary" added to ViewMode plus a "Boundaries" entry in the view selector.

Wiring — src/main.ts holds a BoundaryField (allocated in rebuild alongside the back buffer) and a classify() helper. classify() runs inside reassignPlates (so it covers world build, seed placement and clear-faults) and again in the sim tick after the buffer copy, so boundaries follow the advected plateId. recolour passes boundaries.cellKind to the renderer; colourMode() already forwards "boundary" unchanged.

A few decisions worth flagging:

- Both representations are provided rather than picking one, because per-arc and per-cell are each the cheapest fit for a different consumer (per-arc for mass-flux geology, per-cell for rendering / elevation response). They stay in sync because the per-cell summary is derived in the same pass.
- arcClosing stores the signed normal speed (not just the kind) so 06 gets a magnitude to drive relief height without recomputing velocities.
- The transform split (|vn| vs |vt|, ~45 deg) and the inactive-boundary epsilon are exposed as consts in boundaries.ts for later tuning.
