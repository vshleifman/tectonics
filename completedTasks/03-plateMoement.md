Implemented movement-only plate tectonics with emergent boundaries, exactly as planned.

Rotation maths — added rotateAboutAxis (Rodrigues' formula) to src/math/vec3.ts, with a zero-angle guard so stationary plates pass through untouched.
Advection — new src/sim/advect.ts with nearestCell (greedy hill-climb on dot(position, target)) and advect, a semi-Lagrangian backward gather: each cell traces back along its current plate's rotation and copies all crust fields including plateId from the source cell, so boundaries travel with the crust.
Step — widened src/sim/step.ts to step(mesh, src, dst, plateData, dtMyr), delegating to advect while keeping the pure-over-typed-arrays contract.
Wiring — src/main.ts now holds a backBuffer (rebuilt with the world), a DT_MYR = 2 constant, and a running-gated tick that steps, copies the back buffer back over cellData in place (keeping all existing references valid), then recolours, refreshes velocity arrows and labels.
UI — added a "Run simulation" toggle (default off) and onRunChange callback to src/ui/controls.ts.
Deferred to the next increment (as scoped): boundary classification, elevation response, conservation tracking, and moving the sim into a Web Worker.
