What I implemented
Plates now carry the properties they'll need to move and interact, without any movement logic yet.

Data model (hybrid, as planned)

Per-plate motion in new src/plates/PlateData.ts: angular velocity omega (a Vec3 whose direction is the Euler pole and magnitude is the spin rate in rad/Myr). linearVelocity(plate, pos) derives the per-cell tangent velocity as omega x pos, and centroids(mesh, data) gives a per-plate anchor for arrows.
Per-cell crust in src/data/CellData.ts: crustType (0 oceanic / 1 continental), age (Myr), and density (g/cm3) derived from crust type + age — continental ~2.7 (buoyant), oceanic 2.9→3.0 as it ages (so old oceanic crust is densest and would subduct).
Initialisation — initialisePlateProperties re-derives everything after each plate (re)assignment. Each plate's RNG is keyed by its smallest member cell id, so a plate that survives a re-crack keeps the same pole/speed/crust type rather than flickering; per-cell age is keyed by cell id so it's stable too.

Wiring — src/main.ts holds a plateData and calls reinitPlateProperties() after every assignPlates() (rebuild, seed placement, clear faults).

Visualisation — src/render/CellRenderer.ts adds "crust" and "density" colour modes plus a per-plate velocity-arrow overlay (updateVelocities / setVelocitiesVisible). src/ui/controls.ts exposes the two new view modes and a "Show plate velocities" toggle (off by default).

A few decisions worth flagging:

I represented velocity as an Euler-pole angular velocity rather than a flat tangent vector — it's the only representation that stays consistent across an extended plate on a sphere, and it folds your "rotation should be considered" requirement in for free.
Density is derived rather than independently authored, so it can't drift out of sync with crust type/age. The bounds (DENSITY_MIN/MAX) are exported so the renderer's ramp matches.
The arrow/speed scale constants (VELOCITY_SCALE, speed range) are tuned for visibility, not real-world cm/yr — easy to recalibrate once movement is implemented.
One known limitation I kept (per the plan): because Plates.assignPlates() renumbers plates on every flood fill, plate identity still isn't truly stable across re-cracks — the min-cell-id keying just minimises the visible churn. When you build movement, you'll likely want persistent plate ids.
