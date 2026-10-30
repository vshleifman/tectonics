//! What can be found where. Each resource is a rule over the history of the
//! rock at a cell's surface: what it is made of, when it last had arc
//! magmatism and how far erosion has cut since, how hot it has been, and how
//! old and deep-rooted the crust is. Potentials are derived each tick, never
//! stored as state, so a rule can change without touching the sim, the save
//! format or the transport.
//!
//! A potential is in [0, 1] and uses soft ramps rather than cut-offs, so the
//! map shows gradients and grid noise does not speckle it. Each rule's setting
//! thresholds are in [`Tuning`]; the ramps on rock makeup below are shared and
//! only say how much of a rock type counts as "high".
//!
//! Not here, for want of a memory of past rifting (`rift_side` is cleared once
//! a rift stops): carbonatite rare earths and SEDEX lead-zinc. Not here, for
//! want of climate: coal, oil and gas, evaporites and potash, bauxite, laterite
//! nickel, carbonate-hosted lead-zinc, phosphorite and placers.

use crate::cell_data::{
    CONTINENTAL, CellData, LANE_ARC_EXHUMED, LANE_COVER_FELSIC, LANE_COVER_MAFIC, LANE_EXHUMED,
    LANE_FELSIC, LANE_MAFIC, LANE_PEAK_TEMP, LANE_PLUME, LANE_SEDIMENT,
};
use crate::climate::{Climate, ClimateSample};
use crate::par;
use crate::tuning::{CRATON_KEEL, Tuning};

/// A kind of deposit, numbered as in the resources buffer and the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Resource {
    PorphyryCopper = 0,
    EpithermalGold,
    VolcanogenicMassiveSulphide,
    MagmaticNickel,
    PodiformChromite,
    OrogenicGold,
    GraniteTinTungsten,
    KimberliteDiamonds,
}

/// How many resources there are.
pub const RESOURCES: usize = 8;

/// Display names, in the order of [`Resource`].
pub const RESOURCE_NAMES: [&str; RESOURCES] = [
    "Porphyry Cu-Mo-Au",
    "Epithermal Au-Ag",
    "VMS Cu-Zn",
    "Magmatic Ni-Cu-PGE",
    "Podiform chromite",
    "Orogenic gold",
    "Granite Sn-W-Li",
    "Kimberlite diamonds",
];

/// Share of a rock type in the crust below which it does not count, and above
/// which it counts in full: the makeup ramps every rule shares.
const MAFIC_RAMP: (f64, f64) = (0.5, 0.8);
const FELSIC_RAMP: (f64, f64) = (0.15, 0.4);
const METASEDIMENT_RAMP: (f64, f64) = (0.2, 0.5);
/// Share of plume crust that marks a large igneous province.
const PLUME_RAMP: (f64, f64) = (0.3, 0.7);
/// Gold-bearing mafic rock and metasediment count at a lower share than the
/// ramps above, since either will host it.
const GOLD_HOST_RAMP: (f64, f64) = (0.3, 0.6);
/// How wide a threshold's soft edge is, as a share of the threshold.
const EDGE: f64 = 0.25;

/// Everything about one cell that a rule reads.
#[derive(Clone, Copy, Debug, Default)]
pub struct CellView {
    pub continental: bool,
    pub mafic: f64,
    pub felsic: f64,
    /// Sediment that is no longer loose cover: buried into the basement.
    pub metasediment: f64,
    pub plume: f64,
    /// Myr since the crust last took in arc crust; `f64::MAX` for never.
    pub arc_age: f64,
    /// Km of basement eroded since that arc magmatism.
    pub dug_since_arc: f64,
    /// Hottest (°C) the surface rock has been.
    pub peak_temp: f64,
    pub keel: f64,
    /// Age (Myr) of the crust.
    pub age: f64,
}

impl CellView {
    pub fn of(data: &CellData, i: usize) -> CellView {
        let lane = |l: usize| data.lanes.lane(l)[i] as f64;
        let cover = lane(LANE_COVER_FELSIC) + lane(LANE_COVER_MAFIC);
        CellView {
            continental: data.crust_type[i] == CONTINENTAL,
            mafic: lane(LANE_MAFIC),
            felsic: lane(LANE_FELSIC),
            metasediment: (lane(LANE_SEDIMENT) - cover).max(0.0),
            plume: lane(LANE_PLUME),
            arc_age: data.arc_age[i] as f64,
            dug_since_arc: (lane(LANE_EXHUMED) - lane(LANE_ARC_EXHUMED)).max(0.0),
            peak_temp: lane(LANE_PEAK_TEMP),
            keel: data.keel[i] as f64,
            age: data.age[i] as f64,
        }
    }
}

/// 0 below `lo`, 1 above `hi`, smooth between.
fn ramp(x: f64, (lo, hi): (f64, f64)) -> f64 {
    if hi <= lo {
        return (x >= hi) as u8 as f64;
    }
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 1 well above `limit`, 0 well below it, half at it.
fn above(x: f64, limit: f64) -> f64 {
    let edge = EDGE * limit.abs();
    ramp(x, (limit - edge, limit + edge))
}

/// 1 well below `limit`, 0 well above it, half at it.
fn below(x: f64, limit: f64) -> f64 {
    1.0 - above(x, limit)
}

fn arc_is_young(v: &CellView, t: &Tuning) -> f64 {
    1.0 - ramp(v.arc_age, (t.resource_arc_age_full, t.resource_arc_age_zero))
}

/// Cu, Mo and Au in the roots of an arc volcano, 1-5 km down when it formed
/// (Sillitoe 2010, *Porphyry copper systems*, Economic Geology 105): only
/// found once erosion has cut that deep, and gone once it has cut deeper.
fn porphyry(v: &CellView, t: &Tuning, _: Option<ClimateSample>) -> f64 {
    ramp(v.felsic, FELSIC_RAMP)
        * arc_is_young(v, t)
        * above(v.dug_since_arc, t.porphyry_min_depth)
        * below(v.dug_since_arc, t.porphyry_max_depth)
}

/// Au and Ag from hot water near the surface of the same arc, which erosion
/// removes first (Hedenquist and Lowenstern 1994, Nature 370).
fn epithermal(v: &CellView, t: &Tuning, _: Option<ClimateSample>) -> f64 {
    ramp(v.felsic, FELSIC_RAMP)
        * arc_is_young(v, t)
        * below(v.dug_since_arc, t.epithermal_max_depth)
}

/// Cu and Zn deposited by hot springs on the sea floor (Franklin et al. 2005,
/// Economic Geology 100th Anniversary Volume): in mafic crust still young at a
/// spreading ridge, or in mafic crust accreted onto a continent, either way
/// not cooked past low grade.
fn volcanogenic_massive_sulphide(v: &CellView, t: &Tuning, _: Option<ClimateSample>) -> f64 {
    let young_floor = (!v.continental) as u8 as f64 * below(v.age, t.vms_max_age);
    let accreted = v.continental as u8 as f64;
    ramp(v.mafic, MAFIC_RAMP) * young_floor.max(accreted) * below(v.peak_temp, t.vms_max_temp)
}

/// Ni, Cu and platinum-group metals in the sulphide-rich intrusions of a large
/// igneous province (Naldrett 2004, *Magmatic Sulfide Deposits*).
fn magmatic_nickel(v: &CellView, _: &Tuning, _: Option<ClimateSample>) -> f64 {
    ramp(v.plume, PLUME_RAMP) * ramp(v.mafic, MAFIC_RAMP)
}

/// Chromite in the mantle part of an ophiolite (Arai 1997; Stowe 1994), which
/// here shows as accreted mafic crust inside a continent that no plume made.
fn podiform_chromite(v: &CellView, t: &Tuning, _: Option<ClimateSample>) -> f64 {
    v.continental as u8 as f64
        * ramp(v.mafic, MAFIC_RAMP)
        * (1.0 - ramp(v.plume, PLUME_RAMP))
        * below(v.peak_temp, t.chromite_max_temp)
}

/// Gold from metamorphic fluids in greenschist to amphibolite belts, in mafic
/// rock or metasediment (Groves et al. 1998, Ore Geology Reviews 13).
fn orogenic_gold(v: &CellView, t: &Tuning, _: Option<ClimateSample>) -> f64 {
    v.continental as u8 as f64
        * ramp(v.mafic.max(v.metasediment), GOLD_HOST_RAMP)
        * above(v.peak_temp, t.gold_min_temp)
        * below(v.peak_temp, t.gold_max_temp)
}

/// Sn, W and Li from granite that melted sedimentary crust (Černý et al. 2005,
/// Economic Geology 100th Anniversary Volume; Romer and Kroner 2016).
fn granite_tin_tungsten(v: &CellView, t: &Tuning, _: Option<ClimateSample>) -> f64 {
    v.continental as u8 as f64
        * ramp(v.metasediment, METASEDIMENT_RAMP)
        * ramp(v.felsic, FELSIC_RAMP)
        * above(v.peak_temp, t.granite_min_temp)
}

/// Diamonds carried up from the cold root of an old craton (Gurney et al.
/// 2010, Economic Geology 105; Janse 2007).
fn kimberlite_diamonds(v: &CellView, t: &Tuning, _: Option<ClimateSample>) -> f64 {
    v.continental as u8 as f64
        * ramp(v.keel, (CRATON_KEEL as f64 - 0.1, CRATON_KEEL as f64))
        * above(v.age, t.kimberlite_min_age)
}

/// A rule: how likely `resource` is to be found at a cell. One that needs
/// climate is skipped while there is none.
pub struct Rule {
    pub resource: Resource,
    pub needs_climate: bool,
    pub f: fn(&CellView, &Tuning, Option<ClimateSample>) -> f64,
}

/// Every rule, in the order of [`Resource`].
pub const RULES: [Rule; RESOURCES] = [
    Rule { resource: Resource::PorphyryCopper, needs_climate: false, f: porphyry },
    Rule { resource: Resource::EpithermalGold, needs_climate: false, f: epithermal },
    Rule {
        resource: Resource::VolcanogenicMassiveSulphide,
        needs_climate: false,
        f: volcanogenic_massive_sulphide,
    },
    Rule { resource: Resource::MagmaticNickel, needs_climate: false, f: magmatic_nickel },
    Rule { resource: Resource::PodiformChromite, needs_climate: false, f: podiform_chromite },
    Rule { resource: Resource::OrogenicGold, needs_climate: false, f: orogenic_gold },
    Rule { resource: Resource::GraniteTinTungsten, needs_climate: false, f: granite_tin_tungsten },
    Rule { resource: Resource::KimberliteDiamonds, needs_climate: false, f: kimberlite_diamonds },
];

/// Fill `out` (resource-major, `RESOURCES * n`) with every cell's potential
/// from `rules`; a resource with no rule, or whose rule needs a climate there
/// is none of, stays at 0.
pub fn evaluate(
    rules: &[Rule],
    data: &CellData,
    climate: &Climate,
    t: &Tuning,
    out: &mut [f32],
) {
    let n = data.thickness.len();
    out.fill(0.0);
    for rule in rules {
        if rule.needs_climate && climate.sample(0).is_none() {
            continue;
        }
        let slot = &mut out[rule.resource as usize * n..(rule.resource as usize + 1) * n];
        par::fill(slot, |i| {
            (rule.f)(&CellView::of(data, i), t, climate.sample(i)).clamp(0.0, 1.0) as f32
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> Tuning {
        Tuning::default()
    }

    fn arc() -> CellView {
        CellView {
            continental: true,
            felsic: 0.6,
            mafic: 0.4,
            arc_age: 10.0,
            dug_since_arc: 3.0,
            peak_temp: 100.0,
            ..CellView::default()
        }
    }

    fn never() -> f64 {
        f64::MAX
    }

    #[test]
    fn porphyry_needs_an_arc_cut_into_to_the_right_depth() {
        let t = t();
        assert!(porphyry(&arc(), &t, None) > 0.95);
        // Not yet cut down to it, and cut past it.
        assert_eq!(porphyry(&CellView { dug_since_arc: 0.0, ..arc() }, &t, None), 0.0);
        assert_eq!(porphyry(&CellView { dug_since_arc: 7.0, ..arc() }, &t, None), 0.0);
        // Long since magmatism, or never any, or no felsic rock.
        assert_eq!(porphyry(&CellView { arc_age: 300.0, ..arc() }, &t, None), 0.0);
        assert_eq!(porphyry(&CellView { arc_age: never(), ..arc() }, &t, None), 0.0);
        assert_eq!(porphyry(&CellView { felsic: 0.0, mafic: 1.0, ..arc() }, &t, None), 0.0);
    }

    #[test]
    fn epithermal_is_shallow_and_gone_once_cut_deeper() {
        let t = t();
        let fresh = CellView { dug_since_arc: 0.2, ..arc() };
        assert!(epithermal(&fresh, &t, None) > 0.95);
        assert_eq!(epithermal(&CellView { dug_since_arc: 3.0, ..arc() }, &t, None), 0.0);
        assert_eq!(epithermal(&CellView { arc_age: never(), ..fresh }, &t, None), 0.0);
    }

    #[test]
    fn vms_is_young_mafic_sea_floor_or_accreted_mafic_crust_at_low_grade() {
        let t = t();
        let floor = CellView { mafic: 1.0, age: 5.0, ..CellView::default() };
        assert!(volcanogenic_massive_sulphide(&floor, &t, None) > 0.95);
        // Old ocean floor, felsic crust and cooked rock have none.
        assert_eq!(volcanogenic_massive_sulphide(&CellView { age: 150.0, ..floor }, &t, None), 0.0);
        assert_eq!(volcanogenic_massive_sulphide(&CellView { mafic: 0.1, ..floor }, &t, None), 0.0);
        assert_eq!(volcanogenic_massive_sulphide(&CellView { peak_temp: 700.0, ..floor }, &t, None), 0.0);
        // Accreted into a continent, age no longer matters.
        let accreted = CellView { continental: true, age: 800.0, ..floor };
        assert!(volcanogenic_massive_sulphide(&accreted, &t, None) > 0.95);
    }

    #[test]
    fn magmatic_nickel_needs_plume_made_mafic_crust() {
        let t = t();
        let lip = CellView { plume: 1.0, mafic: 1.0, ..CellView::default() };
        assert!(magmatic_nickel(&lip, &t, None) > 0.95);
        assert_eq!(magmatic_nickel(&CellView { plume: 0.0, ..lip }, &t, None), 0.0);
        assert_eq!(magmatic_nickel(&CellView { mafic: 0.2, ..lip }, &t, None), 0.0);
    }

    #[test]
    fn chromite_is_accreted_ocean_crust_in_a_continent_with_no_plume() {
        let t = t();
        let ophiolite = CellView { continental: true, mafic: 1.0, peak_temp: 300.0, ..CellView::default() };
        assert!(podiform_chromite(&ophiolite, &t, None) > 0.95);
        assert_eq!(podiform_chromite(&CellView { continental: false, ..ophiolite }, &t, None), 0.0);
        assert_eq!(podiform_chromite(&CellView { plume: 1.0, ..ophiolite }, &t, None), 0.0);
        assert_eq!(podiform_chromite(&CellView { peak_temp: 900.0, ..ophiolite }, &t, None), 0.0);
    }

    #[test]
    fn orogenic_gold_is_a_greenschist_to_amphibolite_belt_in_the_right_host() {
        let t = t();
        let belt = CellView { continental: true, mafic: 0.8, peak_temp: 450.0, ..CellView::default() };
        assert!(orogenic_gold(&belt, &t, None) > 0.95);
        assert_eq!(orogenic_gold(&CellView { peak_temp: 100.0, ..belt }, &t, None), 0.0);
        assert_eq!(orogenic_gold(&CellView { peak_temp: 900.0, ..belt }, &t, None), 0.0);
        assert_eq!(orogenic_gold(&CellView { mafic: 0.0, ..belt }, &t, None), 0.0);
        // Metasediment hosts it as well.
        let silt = CellView { mafic: 0.0, metasediment: 0.8, ..belt };
        assert!(orogenic_gold(&silt, &t, None) > 0.95);
    }

    #[test]
    fn tin_granite_is_felsic_melt_of_hot_metasediment() {
        let t = t();
        let granite = CellView {
            continental: true,
            felsic: 0.5,
            metasediment: 0.5,
            peak_temp: 800.0,
            ..CellView::default()
        };
        assert!(granite_tin_tungsten(&granite, &t, None) > 0.95);
        assert_eq!(granite_tin_tungsten(&CellView { peak_temp: 400.0, ..granite }, &t, None), 0.0);
        assert_eq!(granite_tin_tungsten(&CellView { metasediment: 0.0, ..granite }, &t, None), 0.0);
        assert_eq!(granite_tin_tungsten(&CellView { felsic: 0.0, ..granite }, &t, None), 0.0);
    }

    #[test]
    fn diamonds_need_an_old_craton() {
        let t = t();
        let craton = CellView { continental: true, keel: 0.9, age: 2500.0, ..CellView::default() };
        assert!(kimberlite_diamonds(&craton, &t, None) > 0.95);
        assert_eq!(kimberlite_diamonds(&CellView { age: 500.0, ..craton }, &t, None), 0.0);
        assert_eq!(kimberlite_diamonds(&CellView { keel: 0.2, ..craton }, &t, None), 0.0);
        assert_eq!(kimberlite_diamonds(&CellView { continental: false, ..craton }, &t, None), 0.0);
    }

    #[test]
    fn the_rules_are_in_the_order_of_the_enum_and_potentials_are_ramps_not_steps() {
        for (i, rule) in RULES.iter().enumerate() {
            assert_eq!(rule.resource as usize, i);
        }
        let t = t();
        let at = |dug: f64| porphyry(&CellView { dug_since_arc: dug, ..arc() }, &t, None);
        let edge = at(t.porphyry_min_depth);
        assert!(edge > 0.0 && edge < 1.0, "{edge}");
        assert!(at(0.8) < at(1.0) && at(1.0) < at(1.4));
    }

    #[test]
    fn a_rule_that_needs_climate_never_fires_without_one() {
        fn always(_: &CellView, _: &Tuning, _: Option<ClimateSample>) -> f64 {
            1.0
        }
        let rules = [
            Rule { resource: Resource::PorphyryCopper, needs_climate: true, f: always },
            Rule { resource: Resource::EpithermalGold, needs_climate: false, f: always },
        ];
        let data = CellData::new(4);
        let mut out = vec![9.0f32; RESOURCES * 4];
        evaluate(&rules, &data, &Climate::default(), &t(), &mut out);
        assert!(out[..4].iter().all(|&x| x == 0.0), "the climate rule fired");
        assert!(out[4..8].iter().all(|&x| x == 1.0), "the plain rule did not");
        // With a climate, it does.
        let climate = Climate {
            temperature: Some(vec![15.0; 4]),
            runoff: Some(vec![1.0; 4]),
        };
        evaluate(&rules, &data, &climate, &t(), &mut out);
        assert!(out[..4].iter().all(|&x| x == 1.0));
    }
}
