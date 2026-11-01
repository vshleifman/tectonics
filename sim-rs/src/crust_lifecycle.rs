//! Crust creation and ageing, mirroring `crustLifecycle.ts`: rift gaps are
//! filled with fresh ocean (and booked as created mass), everything else ages,
//! oceanic crust thickened past the threshold turns continental, and continental
//! crust thinned below the revert thickness turns back into ocean.

use crate::advect::AdvectionField;
use crate::cell_data::{
    CONTINENTAL, CellData, FRESH_FLOOR, LANE_COVER_MAFIC, LANE_FELSIC, LANE_MAFIC, LANE_PLUME,
    LANE_SEDIMENT, LANE_SEDIMENT_FELSIC, LANES, Mix, NEVER, OCEANIC,
    clamp_lane, is_cover, total_crust_mass,
};
use crate::geotherm::temperature_at;
use crate::math::clamp01;
use crate::elevation::{column_height, eclogite_km};
use crate::lithosphere::neutral_age;
use crate::mesh::Mesh;
use crate::par::{self, Disjoint};
use crate::scratch::Scratch;
use crate::tuning::Tuning;

const SEEN: u8 = 1;
const RIM: u8 = 2;
/// Enclosed bodies of ocean this many cells or fewer fill in a single step.
const PINHOLE_CELLS: usize = 2;

/// Fill small bodies of ocean trapped inside a continent with crust shed off
/// the land around them. Arcs that coalesce around a patch of sea floor leave
/// it ringed by the continent they build, where no trench can reach it, so it
/// would otherwise ride inside that continent for ever. A body only counts if
/// it is under `basin_infill_max_area` of the globe and lies wholly within one
/// plate, away from any boundary, so a young ocean opening between two plates
/// is never filled. Its rim gives up crust above the continental threshold,
/// and nothing is created or lost.
pub fn infill_enclosed_basins(
    mesh: &Mesh,
    data: &mut CellData,
    dt_myr: f64,
    field: &mut AdvectionField,
    s: &mut Scratch,
    t: &Tuning,
) {
    field.continentalised_by_infill = 0.0;
    let rate = t.basin_infill_rate * dt_myr;
    if rate <= 0.0 || t.basin_infill_max_area <= 0.0 {
        return;
    }
    let n = mesh.cell_count;
    let limit = t.basin_infill_max_area * mesh.areas.iter().map(|&a| a as f64).sum::<f64>();
    let threshold = t.continental_thickness_threshold;
    let mark = &mut s.u8a;
    let basin = &mut s.queue;
    let mut rim: Vec<usize> = Vec::new();
    let mut infilled = 0.0f64;

    for start in 0..n {
        if mark[start] != 0 || data.crust_type[start] == CONTINENTAL {
            continue;
        }
        let plate = data.plate_id[start];
        mark[start] = SEEN;
        basin[0] = start as i32;
        let (mut head, mut tail) = (0usize, 1usize);
        let mut area = 0.0f64;
        let mut enclosed = true;
        rim.clear();
        while head < tail {
            let cell = basin[head] as usize;
            head += 1;
            area += mesh.area(cell);
            // A rift opened this step is about to be laid as fresh ocean
            // floor, which would overwrite any crust shed into it.
            if field.rift_cell[cell] != 0 {
                enclosed = false;
            }
            for k in mesh.nbrs(cell) {
                let m = mesh.nbr[k] as usize;
                if data.plate_id[m] != plate {
                    enclosed = false;
                }
                if data.crust_type[m] == CONTINENTAL {
                    if mark[m] == 0 {
                        mark[m] = RIM;
                        rim.push(m);
                    }
                } else if mark[m] != SEEN {
                    mark[m] = SEEN;
                    basin[tail] = m as i32;
                    tail += 1;
                }
            }
        }
        for &m in &rim {
            mark[m] = 0;
        }
        if !enclosed || area >= limit {
            continue;
        }

        let cells = &basin[..tail];
        // A pinhole of a cell or two closes at once, its rim giving down to
        // the revert thickness: the rim is inland, so it stays continental.
        let pinhole = cells.len() <= PINHOLE_CELLS;
        let (step, rim_floor) = if pinhole {
            (f64::INFINITY, (t.continental_revert_thickness + 1e-3).min(threshold))
        } else {
            (rate, threshold)
        };
        let wanted: f64 = cells
            .iter()
            .map(|&c| {
                let c = c as usize;
                (threshold - data.thickness[c] as f64).clamp(0.0, step) * mesh.area(c)
            })
            .sum();
        let available: f64 = rim
            .iter()
            .map(|&m| (data.thickness[m] as f64 - rim_floor).max(0.0) * mesh.area(m))
            .sum();
        if wanted <= 0.0 || available <= 0.0 {
            continue;
        }
        let moved = wanted.min(available);
        // What the rim sheds is deformed on its way in, so its cover stays
        // behind; the rest of its makeup goes into the basin.
        let mut carried = [0.0f64; LANES];
        for &m in &rim {
            let spare = (data.thickness[m] as f64 - rim_floor).max(0.0) * mesh.area(m);
            let take = moved * spare / available;
            data.thickness[m] = (data.thickness[m] as f64 - take / mesh.area(m)) as f32;
            for l in (0..LANES).filter(|&l| !is_cover(l)) {
                carried[l] += take * data.lanes.lane(l)[m] as f64;
            }
        }
        let arriving: Mix = std::array::from_fn(|l| clamp_lane(l, carried[l] / moved.max(1e-12)));
        let share = moved / wanted;
        for &c in cells {
            let c = c as usize;
            let h = data.thickness[c] as f64;
            let gain = (threshold - h).clamp(0.0, step) * share;
            if t.composition_crust_type {
                data.lanes.mix(c, h, gain, &arriving);
            }
            data.thickness[c] = (h + gain) as f32;
            // Wholly enclosed by continent. Where type follows composition it
            // is held to the lower, reverting standard: filled to the
            // threshold with what the rim shed.
            let evolved = !t.composition_crust_type
                || data.lanes.evolved(c) >= t.oceanic_felsic_share;
            if h + gain >= threshold - 1e-4 && evolved {
                data.crust_type[c] = CONTINENTAL;
                infilled += mesh.area(c);
            }
        }
    }
    mark.fill(0);
    field.continentalised_by_infill = infilled;
}

/// Lay fresh ocean floor, `ridge_crust_thickness` thick, in the gaps a step
/// of transport opened, booked as created mass. Ages are brought up to date
/// once a tick, by [`apply_crust_lifecycle`], so floor laid partway through a
/// tick is given `age` short of zero by the rest of the tick still to come.
pub fn lay_new_floor(mesh: &Mesh, data: &mut CellData, field: &mut AdvectionField, age: f64, t: &Tuning) {
    let gaps = par::filter_map(mesh.cell_count, |i| (field.rift_cell[i] != 0).then_some(i));
    let thickness = t.ridge_crust_thickness as f32;
    let elevation = column_height(t, thickness as f64, t.mafic_density, 0.0, 0.0, 0.0, 0.0) as f32;
    let mut created = 0.0f64;
    for i in gaps {
        data.crust_type[i] = OCEANIC;
        data.age[i] = age as f32;
        data.lanes.set(i, FRESH_FLOOR);
        data.arc_age[i] = NEVER;
        data.density[i] = t.mafic_density as f32;
        data.lithosphere[i] = 0.0;
        data.thickness[i] = thickness;
        data.elevation[i] = elevation;
        created += mesh.area(i) * thickness as f64;
    }
    field.created_this_step = created;
    field.created_total += created;
}

/// Thickened crust differentiates: where the base of the crust is hotter than
/// the mafic solidus, it melts, and the felsic melt rises into the crust above.
/// Thick or hot crust turns felsic over time; thin or cold crust does not. It
/// is how plume plateaus and arcs become continent (TTG from thickened basalt;
/// reworked arcs), so it is not limited to crust already called continental,
/// but ocean crust only counts once it is thicker than a ridge lays: a ridge's
/// own crust is thin, and the heat flow it loses is the cooling of new floor,
/// not a hot base. Buried sediment that mafic rock shed (greywacke off an arc
/// or a plateau) melts with it to the same felsic melt; sediment that felsic
/// rock shed already counts as evolved. Only mass moves between the lanes:
/// thickness is untouched. `heat_flow` is the heat each cell loses (mW/m^2),
/// as of the last step, which sets the base temperature by conduction; where
/// an arc has lately been active, magma holds the base at `arc_base_temp`.
pub fn differentiate(data: &mut CellData, heat_flow: &[f32], dt_myr: f64, t: &Tuning) {
    let (thickness, crust_type, arc_age) = (&data.thickness, &data.crust_type, &data.arc_age);
    let lanes = data.lanes.split_mut();
    let ridge = t.ridge_crust_thickness.max(t.oceanic_thickness);
    for i in 0..thickness.len() {
        // Where type follows thickness only continent differentiates; where it
        // follows composition, any crust thickened past a ridge's.
        if crust_type[i] != CONTINENTAL && (!t.composition_crust_type || thickness[i] as f64 <= ridge) {
            continue;
        }
        // Crust an arc has lately built or fed runs hot: basalt ponds at its
        // base, fading over `arc_heat_duration` once the arc stops.
        let base = heat_flow[i] as f64;
        let fresh = if arc_age[i] == NEVER || !t.composition_crust_type {
            0.0
        } else {
            (1.0 - arc_age[i] as f64 / t.arc_heat_duration.max(1e-9)).clamp(0.0, 1.0)
        };
        let conducted = temperature_at(t, base, thickness[i] as f64);
        let moho = conducted + (t.arc_base_temp - conducted).max(0.0) * fresh;
        let melting = clamp01((moho - t.mafic_solidus) / t.melting_span.max(1e-9));
        let rate = clamp01(t.differentiation_rate * melting * dt_myr);
        if rate <= 0.0 {
            continue;
        }
        let moved = rate * lanes[LANE_MAFIC][i] as f64;
        lanes[LANE_MAFIC][i] = (lanes[LANE_MAFIC][i] as f64 - moved) as f32;
        let buried_mafic = (lanes[LANE_SEDIMENT][i] - lanes[LANE_SEDIMENT_FELSIC][i] - lanes[LANE_COVER_MAFIC][i])
            .max(0.0) as f64;
        let melted = rate * buried_mafic;
        lanes[LANE_SEDIMENT][i] = (lanes[LANE_SEDIMENT][i] as f64 - melted) as f32;
        lanes[LANE_FELSIC][i] = (lanes[LANE_FELSIC][i] as f64 + moved + melted) as f32;
    }
}

/// Mafic rock at the base of the crust deeper than `eclogite_depth` has turned
/// to eclogite, denser than the mantle beneath, and founders into it over
/// `eclogite_founder_time`: the dense roots of arcs and orogens drip off (Jull &
/// Kelemen 2001; Jagoutz & Behn 2013). It leaves from the base, so only the
/// mafic lane loses mass and the rest of the column is a larger share of what
/// is left; the records of the rock at the surface are untouched. Books the
/// mass lost as delaminated and gone down.
pub fn founder_eclogite(
    mesh: &Mesh,
    data: &mut CellData,
    dt_myr: f64,
    field: &mut AdvectionField,
    t: &Tuning,
) {
    if t.eclogite_founder_time <= 0.0 {
        return;
    }
    let rate = 1.0 - (-dt_myr / t.eclogite_founder_time).exp();
    let mut lost = 0.0f64;
    for i in 0..data.thickness.len() {
        if field.rift_cell[i] != 0 {
            continue;
        }
        let h = data.thickness[i] as f64;
        let mafic = data.lanes.lane(LANE_MAFIC)[i] as f64;
        let gone = rate * eclogite_km(t, mafic, h);
        if gone <= 0.0 {
            continue;
        }
        let after = h - gone;
        for l in (0..LANES).filter(|&l| crate::cell_data::is_share(l)) {
            let km = data.lanes.lane(l)[i] as f64 * h - if l == LANE_MAFIC { gone } else { 0.0 };
            data.lanes.lane_mut(l)[i] = clamp_lane(l, km / after) as f32;
        }
        data.thickness[i] = after as f32;
        lost += gone * mesh.area(i);
    }
    field.delaminated_this_step += lost;
    field.subducted_total += lost;
}

/// Crust type, by thickness unless `composition_crust_type`; where it follows
/// composition, it follows what the crust is made of. Ocean floor turns continental
/// once the basement is evolved enough (`continental_felsic_share`: bulk
/// continental crust is andesitic) and thick enough to stay so
/// (`continental_revert_thickness`), whatever brought it there, and continent
/// reverts to ocean once thinned to no more than ocean floor, or on its coast
/// once it is less evolved than `oceanic_felsic_share` (below the turning
/// point, so a cell near it does not flicker). Thickness alone no longer makes
/// continent: a thick mafic arc or plateau stays oceanic until it has
/// differentiated (see [`differentiate`]). Flips are decided on the types at the
/// start of the pass, so the order cells are visited in does not matter.
pub fn apply_crust_lifecycle(
    mesh: &Mesh,
    data: &mut CellData,
    dt_myr: f64,
    field: &mut AdvectionField,
    s: &mut Scratch,
    t: &Tuning,
) {
    let areas = &mesh.areas;
    let (mut by_accretion, mut by_spreading, mut decontinentalised) = (0.0f64, 0.0f64, 0.0f64);
    let (mut by_plume, mut by_piling) = (0.0f64, 0.0f64);
    field.piled.fill(0);
    let was = &mut s.u8b;
    par::fill(was, |i| (data.crust_type[i] == CONTINENTAL && field.rift_cell[i] == 0) as u8);
    let was = &*was;
    let continental_nbrs = |i: usize| mesh.nbrs(i).filter(|&k| was[mesh.nbr[k] as usize] != 0).count();
    // Each cell's flip, in cell order, for the tallies.
    enum Flip {
        Plume,
        Piling,
        Spreading,
        Accretion,
        Reverted,
    }
    let flips = {
        let age = Disjoint::new(&mut data.age);
        let crust_type = Disjoint::new(&mut data.crust_type);
        let keel = Disjoint::new(&mut data.keel);
        let arc_age = Disjoint::new(&mut data.arc_age);
        let piled = Disjoint::new(&mut field.piled);
        let thickness = &data.thickness;
        let lanes = &data.lanes;
        let (plume_in, accreted, spread_in) = (&field.plume_in, &field.accreted, &field.spread_in);
        par::filter_map(areas.len(), |i| {
            // Safety: iteration `i` alone touches index `i` of each.
            let flip = unsafe {
                let aged = (age.get(i) as f64 + dt_myr) as f32;
                age.set(i, aged);
                let since_arc = arc_age.get(i);
                if since_arc != NEVER {
                    arc_age.set(i, (since_arc as f64 + dt_myr) as f32);
                }
                let h = thickness[i] as f64;
                let evolved = lanes.evolved(i);
                if crust_type.get(i) == OCEANIC {
                    let nucleates = if t.composition_crust_type {
                        // Basement thick enough to stay continental: loose cover
                        // on a shelf does not make a continent.
                        let basement = h - crate::cell_data::cover_km(lanes, thickness, i);
                        basement >= t.continental_revert_thickness
                            && evolved >= t.continental_felsic_share
                    } else {
                        h >= t.continental_thickness_threshold
                            && (plume_in[i] > 0.0
                                || h >= t.continental_nucleation_thickness
                                || continental_nbrs(i) >= 2)
                    };
                    if !nucleates {
                        return None;
                    }
                    crust_type.set(i, CONTINENTAL);
                    // Where type follows composition, crust a plume mostly made
                    // is credited to it however long ago it was laid: it turns
                    // continental once differentiated. Otherwise whichever
                    // brought in more this step gets the credit; crust spilled
                    // from a full arc cell counts as accretion.
                    Some(if (t.composition_crust_type && lanes.lane(LANE_PLUME)[i] > 0.5)
                        || plume_in[i] > accreted[i].max(spread_in[i])
                    {
                        Flip::Plume
                    } else if accreted[i] <= 0.0 && spread_in[i] <= 0.0 {
                        piled.set(i, 1);
                        Flip::Piling
                    } else if spread_in[i] > accreted[i] {
                        Flip::Spreading
                    } else {
                        Flip::Accretion
                    })
                } else if (h < t.continental_revert_thickness
                    && (continental_nbrs(i) < mesh.nbrs(i).len() || h <= t.oceanic_thickness + 1e-3))
                    || (t.composition_crust_type
                        && evolved < t.oceanic_felsic_share
                        && continental_nbrs(i) < mesh.nbrs(i).len())
                {
                    // Thinned continent is old, cool lithosphere, not crust
                    // fresh from a ridge; but no older than ocean that has yet
                    // to sink, so a margin that loses its crust does not at
                    // once become the side of a trench that goes down.
                    let reverted = (aged as f64).min(neutral_age(t, h));
                    crust_type.set(i, OCEANIC);
                    age.set(i, reverted as f32);
                    keel.set(i, 0.0);
                    Some(Flip::Reverted)
                } else {
                    None
                }
            };
            flip.map(|flip| (i, flip))
        })
    };
    for (i, flip) in flips {
        // Crust thinned to ocean floor has lost what made it continental: it is
        // new ocean floor in all but age, or it would turn straight back.
        if t.composition_crust_type
            && matches!(flip, Flip::Reverted)
            && data.thickness[i] as f64 <= t.oceanic_thickness + 1e-3
        {
            data.lanes.set(i, FRESH_FLOOR);
            data.arc_age[i] = NEVER;
        }
        let tally = match flip {
            Flip::Plume => &mut by_plume,
            Flip::Piling => &mut by_piling,
            Flip::Spreading => &mut by_spreading,
            Flip::Accretion => &mut by_accretion,
            Flip::Reverted => &mut decontinentalised,
        };
        *tally += areas[i] as f64;
    }
    field.continentalised_by_accretion = by_accretion;
    field.continentalised_by_spreading = by_spreading;
    field.continentalised_by_plume = by_plume;
    field.continentalised_by_piling = by_piling;
    field.decontinentalised = decontinentalised;
    field.live_mass = total_crust_mass(areas, data);
    s.u8b.fill(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_data::{LANE_PLUME, LANE_SEDIMENT};

    /// Cells of continent `(thickness km, heat flow mW/m^2)`, all mafic.
    fn continent(cells: &[(f32, f32)]) -> (CellData, Vec<f32>) {
        let mut data = CellData::new(cells.len());
        data.crust_type.fill(CONTINENTAL);
        for (i, &(h, _)) in cells.iter().enumerate() {
            data.thickness[i] = h;
        }
        (data, cells.iter().map(|&(_, q)| q).collect())
    }

    /// Tuning with crust type following composition.
    fn composition() -> Tuning {
        Tuning {
            composition_crust_type: true,
            ..Tuning::default()
        }
    }

    #[test]
    fn new_ocean_floor_is_all_mafic_and_has_seen_no_arc() {
        let mesh = Mesh::icosphere(2);
        let n = mesh.cell_count;
        let mut data = CellData::new(n);
        data.lanes.set(3, [1.0, 1.0, 0.0, 1.0, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0]);
        data.arc_age[3] = 4.0;
        let mut field = AdvectionField::new(n);
        field.rift_cell[3] = 1;
        lay_new_floor(&mesh, &mut data, &mut field, -1.0, &Tuning::default());
        assert_eq!(data.lanes.get(3), FRESH_FLOOR);
        assert_eq!(data.arc_age[3], NEVER);
    }

    #[test]
    fn thick_hot_continent_turns_felsic_and_thin_cold_continent_does_not() {
        let t = Tuning::default();
        // Thick and hot, thin and cold, and thick but insulated from the heat.
        let (mut data, heat) = continent(&[(60.0, 90.0), (30.0, 65.0), (60.0, 40.0)]);
        for _ in 0..100 {
            differentiate(&mut data, &heat, t.dt_myr, &t);
        }
        let felsic = data.lanes.lane(LANE_FELSIC);
        assert!(felsic[0] > 0.1, "thick hot crust only {:.3} felsic", felsic[0]);
        assert_eq!(felsic[1], 0.0, "thin cold crust differentiated");
        assert_eq!(felsic[2], 0.0, "cool crust differentiated");
    }

    #[test]
    fn differentiation_moves_mass_between_lanes_and_leaves_thickness() {
        let t = Tuning::default();
        let (mut data, heat) = continent(&[(60.0, 90.0)]);
        // The buried sediment was shed by felsic rock: it is left as it is.
        data.lanes.set(0, [0.2, 0.1, 0.7, 0.2, 0.0, 0.0, 0.1, 0.0, 0.0, 0.0, 0.0]);
        let before = data.lanes.get(0);
        differentiate(&mut data, &heat, t.dt_myr, &t);
        let after = data.lanes.get(0);
        assert!(after[LANE_FELSIC] > before[LANE_FELSIC]);
        assert!(
            (after[LANE_MAFIC] + after[LANE_FELSIC] - before[LANE_MAFIC] - before[LANE_FELSIC]).abs() < 1e-6
        );
        assert_eq!(after[LANE_PLUME], before[LANE_PLUME]);
        assert_eq!(after[LANE_SEDIMENT], before[LANE_SEDIMENT]);
        assert_eq!(data.thickness[0], 60.0);
        // Shed by mafic rock, it melts with the mafic rock, into the felsic.
        data.lanes.set(0, [0.2, 0.1, 0.7, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        let before = data.lanes.get(0);
        differentiate(&mut data, &heat, t.dt_myr, &t);
        let after = data.lanes.get(0);
        assert!(after[LANE_SEDIMENT] < before[LANE_SEDIMENT]);
        let total = |l: [f32; LANES]| (l[LANE_MAFIC] + l[LANE_FELSIC] + l[LANE_SEDIMENT]) as f64;
        assert!((total(after) - total(before)).abs() < 1e-6);
        assert_eq!(data.thickness[0], 60.0);
    }

    #[test]
    fn ocean_floor_laid_at_a_ridge_does_not_differentiate_however_much_heat_it_loses() {
        let t = Tuning::default();
        // Ridge-thick crust, losing the heat of new floor.
        let (mut data, heat) = continent(&[(t.oceanic_thickness as f32, 490.0)]);
        data.crust_type[0] = OCEANIC;
        for _ in 0..200 {
            differentiate(&mut data, &heat, t.dt_myr, &t);
        }
        assert_eq!(data.lanes.get(0), FRESH_FLOOR);
    }

    #[test]
    fn thickened_ocean_crust_with_a_hot_base_differentiates_and_cold_does_not() {
        let t = composition();
        // A plateau or arc 40 km thick, over hot and over old cold floor.
        let (mut data, heat) = continent(&[(40.0, 90.0), (40.0, 40.0)]);
        data.crust_type.fill(OCEANIC);
        for _ in 0..100 {
            differentiate(&mut data, &heat, t.dt_myr, &t);
        }
        let felsic = data.lanes.lane(LANE_FELSIC);
        assert!(felsic[0] > 0.1, "hot thick ocean crust only {:.3} felsic", felsic[0]);
        assert_eq!(felsic[1], 0.0, "cold thick ocean crust differentiated");
    }

    /// One cell of crust `thickness` km thick, of type `crust_type`, with this
    /// makeup, on a mesh with every other cell ocean floor.
    fn lifecycle_cell(crust_type: u8, thickness: f32, lanes: [f32; LANES]) -> (Mesh, CellData) {
        let mesh = Mesh::icosphere(2);
        let mut data = CellData::new(mesh.cell_count);
        data.crust_type.fill(OCEANIC);
        data.thickness.fill(Tuning::default().oceanic_thickness as f32);
        data.lanes = crate::cell_data::Lanes::fresh_floor(mesh.cell_count);
        data.crust_type[0] = crust_type;
        data.thickness[0] = thickness;
        data.lanes.set(0, lanes);
        (mesh, data)
    }

    fn step(mesh: &Mesh, data: &mut CellData, t: &Tuning) {
        let mut field = AdvectionField::new(mesh.cell_count);
        let mut scratch = Scratch::new(mesh.cell_count);
        apply_crust_lifecycle(mesh, data, t.dt_myr, &mut field, &mut scratch, t);
    }

    #[test]
    fn an_eclogite_root_founders_from_the_base_and_a_shallow_one_does_not() {
        let t = Tuning::default();
        // 60 km, half felsic over half mafic: the mafic below 45 km is eclogite.
        let mut l = lanes_with_felsic(0.5);
        l[LANE_SEDIMENT] = 0.0;
        let (mesh, mut data) = lifecycle_cell(CONTINENTAL, 60.0, l);
        data.thickness[1] = 35.0;
        data.lanes.set(1, lanes_with_felsic(0.0));
        let mut field = AdvectionField::new(mesh.cell_count);
        for _ in 0..200 {
            founder_eclogite(&mesh, &mut data, t.dt_myr, &mut field, &t);
        }
        let h = data.thickness[0] as f64;
        assert!((h - t.eclogite_depth).abs() < 0.5, "root left at {h:.1} km");
        let felsic_km = data.lanes.lane(LANE_FELSIC)[0] as f64 * h;
        assert!((felsic_km - 30.0).abs() < 1e-3, "felsic went down: {felsic_km:.3} km");
        // Mafic crust shallower than eclogite stays.
        assert_eq!(data.thickness[1], 35.0);
    }

    fn lanes_with_felsic(felsic: f32) -> [f32; LANES] {
        let mut l = FRESH_FLOOR;
        l[LANE_MAFIC] = 1.0 - felsic;
        l[LANE_FELSIC] = felsic;
        l
    }

    #[test]
    fn a_thick_mafic_arc_stays_oceanic_until_it_has_differentiated() {
        let t = composition();
        let (mesh, mut data) = lifecycle_cell(OCEANIC, 30.0, lanes_with_felsic(0.1));
        step(&mesh, &mut data, &t);
        assert_eq!(data.crust_type[0], OCEANIC, "thick but mafic crust turned continental");
        data.lanes.set(0, lanes_with_felsic(0.4));
        step(&mesh, &mut data, &t);
        assert_eq!(data.crust_type[0], CONTINENTAL, "evolved crust stayed oceanic");
    }

    #[test]
    fn a_thick_hot_mafic_plateau_differentiates_into_continent_and_stays() {
        let t = composition();
        let (mesh, mut data) = lifecycle_cell(OCEANIC, 30.0, FRESH_FLOOR);
        let heat = vec![90.0f32; mesh.cell_count];
        let mut turned = None;
        for tick in 0..2000 {
            differentiate(&mut data, &heat, t.dt_myr, &t);
            step(&mesh, &mut data, &t);
            if data.crust_type[0] == CONTINENTAL {
                turned = Some(tick);
                break;
            }
        }
        let turned = turned.expect("a hot 30 km plateau never became continent");
        assert!(turned > 20, "turned continental after only {turned} ticks");
        // It stays continental while it is still differentiating.
        for _ in 0..50 {
            differentiate(&mut data, &heat, t.dt_myr, &t);
            step(&mesh, &mut data, &t);
        }
        assert_eq!(data.crust_type[0], CONTINENTAL);
    }

    #[test]
    fn a_thick_arc_over_old_floor_differentiates_while_an_arc_is_active_and_not_after() {
        let t = composition();
        // 35 km of arc crust over old floor losing only 45 mW/m2.
        let (mut data, heat) = continent(&[(35.0, 45.0), (35.0, 45.0), (35.0, 45.0)]);
        data.crust_type.fill(OCEANIC);
        data.arc_age[0] = 0.0; // building now
        data.arc_age[1] = 2.0 * t.arc_heat_duration as f32; // long extinct
        // cell 2 has never had an arc
        for _ in 0..100 {
            differentiate(&mut data, &heat, t.dt_myr, &t);
        }
        let felsic = data.lanes.lane(LANE_FELSIC);
        assert!(felsic[0] > 0.1, "an active arc only {:.3} felsic", felsic[0]);
        assert_eq!(felsic[1], 0.0, "an extinct arc differentiated");
        assert_eq!(felsic[2], 0.0, "crust that never had an arc differentiated");
    }

    #[test]
    fn sediment_cover_on_ocean_floor_does_not_make_it_continental() {
        let t = composition();
        let mut l = FRESH_FLOOR;
        l[crate::cell_data::LANE_SEDIMENT] = 0.5;
        l[LANE_MAFIC] = 0.5;
        l[crate::cell_data::LANE_COVER_FELSIC] = 0.5;
        l[crate::cell_data::LANE_SEDIMENT_FELSIC] = 0.5;
        let (mesh, mut data) = lifecycle_cell(OCEANIC, 30.0, l);
        step(&mesh, &mut data, &t);
        assert_eq!(data.crust_type[0], OCEANIC);
    }

    #[test]
    fn a_cell_near_the_threshold_does_not_flicker() {
        let t = composition();
        // Between the two thresholds on a coast: oceanic stays oceanic and
        // continental stays continental.
        let between = ((t.continental_felsic_share + t.oceanic_felsic_share) / 2.0) as f32;
        for kind in [OCEANIC, CONTINENTAL] {
            let (mesh, mut data) = lifecycle_cell(kind, 30.0, lanes_with_felsic(between));
            for _ in 0..50 {
                step(&mesh, &mut data, &t);
                assert_eq!(data.crust_type[0], kind);
            }
        }
    }

    #[test]
    fn continent_thinned_to_ocean_floor_reverts_with_the_makeup_of_new_floor() {
        let t = composition();
        let (mesh, mut data) =
            lifecycle_cell(CONTINENTAL, t.oceanic_thickness as f32, lanes_with_felsic(0.6));
        data.arc_age[0] = 10.0;
        step(&mesh, &mut data, &t);
        assert_eq!(data.crust_type[0], OCEANIC);
        assert_eq!(data.lanes.get(0), FRESH_FLOOR);
        assert_eq!(data.arc_age[0], NEVER);
        // And it does not turn back.
        step(&mesh, &mut data, &t);
        assert_eq!(data.crust_type[0], OCEANIC);
    }

    #[test]
    fn the_arc_clock_runs_from_an_arc_and_never_starts_without_one() {
        let mesh = Mesh::icosphere(2);
        let n = mesh.cell_count;
        let t = Tuning::default();
        let mut data = CellData::new(n);
        data.thickness.fill(t.oceanic_thickness as f32);
        data.arc_age[7] = 0.0;
        let mut field = AdvectionField::new(n);
        let mut scratch = Scratch::new(n);
        for _ in 0..3 {
            apply_crust_lifecycle(&mesh, &mut data, t.dt_myr, &mut field, &mut scratch, &t);
        }
        assert_eq!(data.arc_age[7], (3.0 * t.dt_myr) as f32);
        assert_eq!(data.arc_age[8], NEVER);
    }
}
