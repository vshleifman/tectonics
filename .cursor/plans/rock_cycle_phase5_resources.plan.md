---
name: "Rock cycle phase 5: resource potential"
overview: "Derive each cell's potential for climate-independent deposit types from composition, arc clock, exhumation, metamorphic peak, keel and age. Rules live in one Rust module, are computed into a derived buffer once a tick, take an optional climate sample, and are reported by the bench rather than tuned to Earth."
todos:
    - id: module
      content: Add resources.rs with a Resource enum, one rule function per resource over a CellView, and a derived potential buffer (cells x resources) refreshed in refresh_surface
      status: pending
    - id: rules
      content: Implement porphyry Cu-Mo-Au, epithermal Au-Ag, VMS Cu-Zn, magmatic Ni-Cu-PGE, ophiolitic chromite, orogenic gold, granite Sn-W-Li and kimberlite diamonds, thresholds as referenced tuning
      status: pending
    - id: climate-hook
      content: Rules take Option<ClimateSample>; register a rule with needs_climate so later climate-dependent rules are skipped until climate exists
      status: pending
    - id: expose
      content: resources_ptr and resource_count exports, a resources view with a resource picker, top three resources in cell labels, resource area metrics in bench and diagnostics
      status: pending
    - id: verify
      content: One unit test per rule on a hand-built cell; dynamics fingerprint unchanged; report resource areas over the run
      status: pending
isProject: false
---

# Rock cycle phase 5: resource potential

Part 5 of 6. Index: [rock_cycle_and_resources.plan.md](rock_cycle_and_resources.plan.md). Depends on phases 2–4.

## Why

This is the point of the work: every cell knows what can be found there. Resources are derived from the rock history and aren't stored as state. The rules can then change without touching the sim, the save format or advection.

## Changes

- A new [sim-rs/src/resources.rs](sim-rs/src/resources.rs):
  - `enum Resource` with a fixed order, and `RESOURCES` as its count;
  - `struct CellView<'a>` borrowing the fields a rule reads;
  - one `fn` per resource, returning a potential in [0, 1];
  - `struct Rule { resource, needs_climate: bool, f }`, with a `RULES` table. `needs_climate` rules are skipped while `Climate` is empty. None are registered in this phase.
- **Buffer.** The potential buffer is lane-major, `RESOURCES * n` `f32`. It lives on `Sim`, is recomputed in `refresh_surface` and at the end of `tick`, and is not serialised: it's derived. One pass over the cells using `par::fill` per resource.
- **Soft thresholds.** Potentials use smooth ramps (`smoothstep` between a lower and an upper bound), not hard cut-offs, so the map shows gradients and grid noise doesn't speckle it.

## Rules

Each threshold is a tuning value with a reference in its doc comment. Ages are in Myr and depths in km.

| Resource | Setting | Inputs |
|---|---|---|
| Porphyry Cu-Mo-Au | Arc magmatism, preserved at 1–5 km depth | Felsic share; `arc_age` young (ramp off over a few tens of Myr); `exhumed - arc_exhumed` between about 1 and 5 km |
| Epithermal Au-Ag | Shallow arc hydrothermal | As porphyry, but erosion since the arc is under about 1.5 km |
| VMS Cu-Zn(-Pb) | Sea-floor volcanic | Mafic share high; ocean crust near spreading (young `age`), or mafic crust preserved inside continent (accreted); low peak grade |
| Magmatic Ni-Cu-PGE | Plume (LIP) intrusions | `LANE_PLUME` share high; mafic share high |
| Podiform chromite | Ophiolite | Continental cell with high mafic share and low plume share, which is how accreted ocean crust shows in this model; low to moderate grade |
| Orogenic gold | Greenschist–amphibolite belts | Peak temperature about 300–600 °C; mafic or metasediment share; continental |
| Sn-W-Li granite | Melting of sedimentary crust | Metasediment share high; peak temperature above about 650 °C; felsic |
| Kimberlite diamonds | Old cratons | `keel` at or above `CRATON_KEEL`; crust `age` above about 1500 Myr |

Two known gaps:
- **Rare earths (carbonatite) and SEDEX lead-zinc** need a memory of past rifting. `rift_side` is cleared once a rift stops. A `rift_age` clock like `arc_age` would add both. Record it as follow-up work rather than widening this phase.
- **Placers** need sediment to carry the resource it came from, and river discharge. They wait for climate.

## Exposure

- [sim-rs/src/lib.rs](sim-rs/src/lib.rs): `resources_ptr`, `resource_count`, and `resource_names` so TypeScript doesn't copy the order by hand.
- A "resources" view in [src/render/colour.ts](src/render/colour.ts) with a picker for which resource to show, as a single-hue ramp. Store the chosen resource in `settingsStore.ts` and `persist.ts`.
- [src/render/CellLabels.ts](src/render/CellLabels.ts): the top three potentials above 0.2.
- Tuning: a "Resources" group in [src/ui/controls.ts](src/ui/controls.ts) with the rule thresholds, mirrored in `tuning.rs` and `tuning.ts`.
- Metrics: per resource, the share of land area and of continental area with potential above 0.5. Add these to the bench and diagnostics.

## Verification

- **One unit test per rule** on a hand-built `CellView`: inside the setting gives potential near 1, and just outside it gives 0. Examples: a porphyry eroded past 5 km scores 0, and kimberlite on a 500 Myr keel scores 0.
- **Climate gate:** a `needs_climate` test rule never fires with `Climate::default()`.
- **Dynamics fingerprint** matches phase 4 bit for bit. The buffer is derived and not part of either fingerprint.
- **Report:** resource areas over the 3000 Myr run. Expected patterns are porphyry along active margins, diamonds only in old craton interiors, and orogenic gold in old sutures. If a pattern is missing, find which input field is missing it before touching a threshold.
