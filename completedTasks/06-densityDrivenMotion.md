What I implemented
Plate motion is no longer arbitrary: each plate's Euler pole now emerges from the crust it carries rather than from a seeded RNG (this was planned as ticket 11, "density-driven plate motion / slab pull"). A plate drifts toward where its dense crust concentrates, refined by slab pull at convergent boundaries and ridge push at divergent ones, recomputed and smoothed every step.

Force model (new src/sim/plateForces.ts)
updatePlateMotion(mesh, data, plateData, boundaries) is a pure function over the mesh interface, flat typed arrays, PlateData and BoundaryField (Worker/WASM-ready, matching step/advect/classify). It mutates each plate's omega in place by summing three torque terms about the sphere centre, then mapping the net torque to a capped, relaxed angular velocity:

- Intrinsic density drive (the bootstrap). Integrating pos * excessDensity * area over each plate gives a density-weighted position; cross(centroid, densePosSum) is the rotation axis that turns the plate toward its dense side, and its magnitude already encodes how much dense crust the plate carries and how lop-sided it is. This needs no boundaries, so motion starts from rest with no chicken-and-egg deadlock against the velocity-derived boundary classification. excessDensity is measured above a continental reference (DENSITY_REF = 2.7), so the drive responds to age/density contrast within the plate.
- Slab pull (convergent arcs). The denser (subducting) cell of each convergent pair is pulled tangentially toward the overriding plate, weighted by its excess density and the arc's great-circle length.
- Ridge push (divergent arcs). Each plate is shoved away from the ridge axis at the midpoint — a weaker term, behind RIDGE_WEIGHT (zeroable to disable).

Torque -> omega: target speed = min(SPEED_GAIN * |tau|, MAX_SPEED) along normalise(tau); omega is then relaxed toward the target each tick (omega += RELAX * (target - omega)) so motion does not jitter as boundaries flicker. A |tau| < EPS guard plus zero-safe normalise keeps stationary/uniform plates at rest rather than producing NaNs. All knobs (DENSITY_REF, INTRINSIC_WEIGHT, SLAB_WEIGHT, RIDGE_WEIGHT, SPEED_GAIN, MAX_SPEED, RELAX, EPS) are module consts.

Plate seeding (src/plates/PlateData.ts)
Retired the random Euler pole: reconcilePlateProperties now starts genuinely new (genesis) plates at rest (omega = [0,0,0]) and lets a rifted fragment inherit its parent's omega (motion continuity) before relaxing toward its own force balance. Removed freshPlate, perturbOmega and the now-unused MIN_SPEED/MAX_SPEED/SPLIT_PERTURB.

Oceanic-first crust (src/plates/PlateData.ts)
A latent bug surfaced once motion depended on crust: initialiseCrust ran while the sphere was still one plate, painting the whole planet with that plate's random continental flag — a continental roll gave a uniform 2.7 density, zero gradient and no motion, and every split inherited the flag. Fixed by seeding every cell as young oceanic crust with an age-driven density (2.9..3.0 across 0..200 Myr), so each plate always has a density gradient to drift along. Continental crust is no longer seeded; it is left to emerge later through accretion at convergent boundaries. The per-plate continental flag (nothing outside PlateData read it — the renderer reads per-cell crustType) and its seeding constants were removed; CONTINENTAL_DENSITY stays only as the buoyant floor of the renderer's density ramp (DENSITY_MIN).

Wiring (src/main.ts)
updatePlateMotion runs in reassignPlates after initialiseCrust (so arrows and boundaries reflect density-driven motion immediately on world build / seed placement, where the intrinsic term works even with empty boundaries) and again at the top of each sim tick before classify/step (recomputing omega from the previous tick's boundaries and the current crust). The existing per-plate velocity arrows now visibly track density/age.

Decisions worth flagging:

- The deterministic density-drive bootstrap was chosen over keeping a tiny random seed pole. It both breaks the at-rest deadlock and ties the very first motion to the crust, so there is no RNG anywhere in the motion path.
- omega is relaxed toward the force target every step rather than set directly, trading instant response for stable, non-jittery motion as boundaries flicker.
- Boundary classification in src/sim/boundaries.ts is untouched: convergent/divergent/transform still emerge purely from relative velocity, now fed by density-driven omega.

Known limitations
The weight/gain constants are reasoned starting values, not empirically calibrated — watch the velocity arrows on a run and raise INTRINSIC_WEIGHT/SPEED_GAIN if drift reads too weak. Continental crust formation (oceanic -> continental via accretion) is deferred to the crust-lifecycle work.
