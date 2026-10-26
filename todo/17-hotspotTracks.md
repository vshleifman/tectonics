# 17 — Hotspot tracks

## Goal

A plume has a head and a tail. The head behaves as now: it rides with the
plate and builds a round plateau. The tail stays fixed in the mantle for tens
of Myr and builds a narrow chain of thickened crust as the plate moves over
it, older down the track, with a broad swell where the tail currently stands.

## Why it matters

Plumes now ride with the plate (`primordial.rs`, "Ride with the plate the
plateau is building on") for their whole 20 Myr life. That is deliberate and
right for Archean plateaus, but it means a modern world has no Hawaii-Emperor,
Louisville or Réunion-Deccan chains. These are among the most recognisable
features of a real ocean floor, and the clearest visible record of plate
motion.

## Design

### Two phases per plume

- **Head**, as now: `plume_duration`, `plume_radius`, rides with the plate,
  peak thickness from `plume_thickness_cold`..`plume_thickness`.
- **Tail**, new: starts where the head was born, in the mantle frame, and
  stays there. The mantle sites and `dyn_topo` are already fixed in mesh
  coordinates, so "fixed" means a fixed point on the mesh.
  - `plume_tail_duration` (default 80 Myr).
  - `plume_tail_radius` (default 0.02 rad, ~130 km). That is about one cell at
    level 6 and about five at level 8, so tracks read as lines from level 7
    up. Document this in the UI note for the field.
  - `plume_tail_thickness` (default +4 km over `ridge_crust_thickness`). The
    target is held below `continental_thickness_threshold - 1` so a tail never
    nucleates continent by itself. The gain is `max(0, target - h)` as for
    heads, so an existing plateau is not thickened further.
  - Uses the same `plume_share = 1` tracer, so its crust is exempt from
    hot-era drip and counts in the plume metrics. Counted in the ledger as
    created.
  - Under continent: it does the same, and because the target is below what
    the continent already has, it adds nothing. A continental track shows only
    as the swell. That is acceptable for now; Yellowstone-style volcanism is
    future work.
- **Swell:** each live tail adds `plume_swell` (default +1.0 km) to `dyn_topo`
  with a cosine taper over `plume_swell_radius` (default 0.1 rad,
  ~640 km), clamped by `dyn_topo_max` along with the rest. It stays put while
  the plate moves off it, so old seamounts subside as they leave the swell,
  which is how the chain deepens with age. With ticket 14 the sea floor's own
  cooling adds to that.

### Implementation notes

- Precompute each tail's footprint, the cells within its radius, once at
  birth. Because the tail is fixed, the footprint never changes. This removes
  the O(n) scan per plume per tick for tails. Heads keep their scan (or get
  the same treatment as a follow-up).
- The swell needs the live tails before `apply_elevation`. `dyn_topo` is
  filled in `drive_plates` (`world.rs`). Either add a
  `primordial.add_swell(&mut dyn_topo)` call right after that fill, or add it
  in `evolve_crust` before the elevation pass. The first keeps `dyn_topo` a
  single readout, so prefer it.
- `Plume { centre, elapsed }` gains `tail_centre: Vec3`,
  `tail_cells: Vec<u32>` and `phase`. The head and the tail can coexist
  briefly. Simplest is for the tail to start at head birth and outlive it.
- Plume rate is unchanged. At `plume_rate` 0.1/Myr times the core factor, and
  80 Myr tails, that is roughly 5-10 live tails late in a run (Earth: about
  20-50 hotspots, depending on who counts). Tune `plume_tail_duration` or the
  rate if the map looks sparse.

## Steps

1. Add the tunings in Rust and TypeScript.
2. Extend `Plume`, birth the tail with its footprint, and thicken under the
   tail each tick.
3. Add the swell to `dyn_topo`.
4. Tests:
   - A fixed tail under a plate rotating at constant omega for 50 Myr leaves
     thickened cells along the small circle the plate traces, at the plate's
     speed, with no thickening off that line.
   - A tail never turns ocean continental.
   - The ledger and tracer test still hold
     (`the_tracer_never_holds_more_plume_crust_than_plumes_made`).
   - The swell is fixed in place and within the clamp.
5. Bench: the primordial-era metrics (`made_plume`, `cratons`,
   `ancient_keeled`) stay in band, since tails add little thickness. Then a
   visual check at level 7.
6. Optional UI: a plume overlay marking live heads and tails.

## Acceptance criteria

- At level 7, a default run late in its history shows visible age-progressive
  chains on oceanic plates, aligned with plate motion.
- Primordial and craton metrics stay inside their bands.
- No ledger drift, and the same fingerprint across thread counts.

## Cost

- Code: small (~150 lines plus tests).
- Runtime: negligible for tails (footprints precomputed), slightly negative
  overall if heads get the same treatment.
- Risk: low. The main risk is tails thickening floor beside continents past
  the threshold, which the target cap prevents.

## Dependencies

None. It looks better with 14, which makes seamounts stand as their thickness
says, and 15, which gives islands a coastline.

## Future

- Let tails drift slowly with the mantle flow (`mantle.rs`) to produce bends
  like the Hawaii-Emperor's.
- Continental hotspot volcanism and uplift.
