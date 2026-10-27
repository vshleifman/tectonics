//! Mantle flow toward downwellings: what gathers continents into
//! supercontinents.
//!
//! Slabs sinking at trenches drag the mantle down with them, and cold slab
//! piles beneath long-lived subduction zones keep it going down for a hundred
//! million years and more after. The upper mantle flows in toward these
//! downwellings from all around, and plates feel that flow as traction on
//! their bases: weakly under ocean, where a soft asthenosphere decouples
//! them, strongly under the deep keels of continents. So continents drift
//! toward where the mantle goes down and pile up there, which is how
//! supercontinents assemble; the heat that then builds beneath them (see
//! `rifting.rs`) breaks them up again.
//!
//! The flow is worked out on a coarse grid of mantle sites (the vertices of a
//! level-3 icosphere, which are the first cells of any finer mesh), so its cost
//! does not grow with the mesh. Each site's downwelling is the rate at which
//! plate has been subducting there, remembered over `downwelling_memory`. Only
//! continental keels are carried by it (see `plate_forces.rs`). The
//! flow is that of point sinks in a thin spherical shell, balanced by gentle
//! upwelling everywhere else: a sink taking in area `Q` per Myr draws the
//! surface toward it at `Q cot(d/2) / 4π` at angular distance `d`, strong near
//! it, weak far off and nothing at its antipode.
//!
//! Each site also carries the temperature of the upper mantle beneath it, as
//! an anomaly (K) about the planet's mantle temperature (see `heat.rs`). Cold
//! slab sinking at a site cools it; a continent over it holds heat in and
//! warms it, as the mantle under a supercontinent warms by ~100 K. Anomalies
//! spread sideways to the neighbouring sites and fade as the upper mantle
//! overturns. Where the mantle runs hot it rises, and the heat that builds
//! under a continent before it rifts (see `rifting.rs`) builds fastest there.
//!
//! Below it lies the lower mantle, which remembers far longer. Slab sunk at a
//! site reaches it over `slab_transit_time` and piles up on the core, as the
//! slab graveyards beneath the old Tethys and the Pacific rim do, spreading
//! slowly and fading over `slab_pile_lifetime` as it warms through. A cold
//! pile chills the upper mantle over it a little. Plumes rise from the core
//! at the edges of the piles, where hot mantle they have pushed aside gathers,
//! as Earth's plumes rise along the rims of its two deep piles (the LLSVPs);
//! warm upper mantle over an edge helps them up, and `plume_floor` lets some
//! rise anywhere, so a mantle with no piles still raises them.
//!
//! The surface feels both layers' motion as dynamic topography (see
//! `elevation.rs`): lifted over hot upper mantle, dragged down over
//! downwellings.

use crate::boundaries::{BoundaryField, CONVERGENT};
use crate::cell_data::{CONTINENTAL, CellData};
use crate::math::Vec3;
use crate::mesh::Mesh;
use crate::par;
use crate::slabs::subduction;
use crate::tuning::{SimOptions, Tuning};

/// Mantle sites: the vertices of a level-3 icosphere.
const SITES: usize = 642;
/// Closest a sink's pull is evaluated, as a multiple of the site spacing, so
/// the flow right over a downwelling stays finite.
const NEAR_SITES: f64 = 1.5;

pub struct MantleFlow {
    /// The mantle site each cell lies over.
    site_of: Vec<u32>,
    sites: Vec<Vec3>,
    /// Area of the cells over each site.
    site_area: Vec<f64>,
    /// The cells over each site, one site's after another's: site `s` has
    /// `site_cells[site_start[s]..site_start[s + 1]]`.
    site_start: Vec<u32>,
    site_cells: Vec<u32>,
    /// The sites bordering each site: those with a cell touching one of its.
    site_nbrs: Vec<Vec<u32>>,
    /// The sites each cell's temperature is blended from, with their weights:
    /// cell `i` has `blend[blend_start[i]..blend_start[i + 1]]`.
    blend_start: Vec<u32>,
    blend: Vec<(u32, f32)>,
    /// Remembered subduction at each site, as area of plate per Myr.
    sink: Vec<f64>,
    /// Upwelling at each site from the heat trapped beneath it, as area per
    /// Myr.
    source: Vec<f64>,
    flux: Vec<f64>,
    /// Flow at the top of the mantle over each site, as a tangent velocity
    /// (rad/Myr).
    wind: Vec<Vec3>,
    /// Temperature of the upper mantle at each site (K), above or below the
    /// planet's mantle temperature.
    upper_temp: Vec<f64>,
    /// `upper_temp_at` for every cell (K), as of the last update.
    cell_temp: Vec<f32>,
    /// Slab sunk at each site and still on its way down, as a share of the
    /// site's area.
    transit: Vec<f64>,
    /// Slab piled in the lower mantle beneath each site, as a share of the
    /// site's area.
    lower_slab: Vec<f64>,
    /// `lower_slab_at` for every cell, as of the last update.
    cell_pile: Vec<f32>,
    /// Running total of the sites' plume weights, site by site: plumes rise
    /// at site `s` in proportion to `plume_total[s] - plume_total[s - 1]`.
    plume_total: Vec<f64>,
    /// Largest site's plume weight over the mean, by area: 1 for plumes
    /// rising anywhere alike.
    plume_peak: f64,
}

impl MantleFlow {
    pub fn new(mesh: &Mesh) -> MantleFlow {
        let n = mesh.cell_count;
        let count = SITES.min(n);
        let sites: Vec<Vec3> = (0..count)
            .map(|i| {
                [
                    mesh.centres[i * 3] as f64,
                    mesh.centres[i * 3 + 1] as f64,
                    mesh.centres[i * 3 + 2] as f64,
                ]
            })
            .collect();
        // Each cell to the site nearest it through the mesh, by a breadth-first
        // search out from every site at once.
        let mut site_of = vec![u32::MAX; n];
        let mut queue: Vec<usize> = (0..count).collect();
        for (i, s) in site_of.iter_mut().enumerate().take(count) {
            *s = i as u32;
        }
        let mut head = 0;
        while head < queue.len() {
            let cell = queue[head];
            head += 1;
            for k in mesh.nbrs(cell) {
                let m = mesh.nbr[k] as usize;
                if site_of[m] == u32::MAX {
                    site_of[m] = site_of[cell];
                    queue.push(m);
                }
            }
        }

        let mut site_area = vec![0.0f64; count];
        let mut site_start = vec![0u32; count + 1];
        for (i, &s) in site_of.iter().enumerate() {
            site_area[s as usize] += mesh.area(i);
            site_start[s as usize + 1] += 1;
        }
        for s in 0..count {
            site_start[s + 1] += site_start[s];
        }
        let mut site_cells = vec![0u32; n];
        let mut write = site_start.clone();
        for (i, &s) in site_of.iter().enumerate() {
            site_cells[write[s as usize] as usize] = i as u32;
            write[s as usize] += 1;
        }

        let mut site_nbrs: Vec<Vec<u32>> = vec![Vec::new(); count];
        for cell in 0..n {
            let s = site_of[cell];
            for k in mesh.nbrs(cell) {
                let other = site_of[mesh.nbr[k] as usize];
                if other != s {
                    site_nbrs[s as usize].push(other);
                }
            }
        }
        for nbrs in &mut site_nbrs {
            nbrs.sort_unstable();
            nbrs.dedup();
        }

        // Each cell blends its own site and the sites around it, each by a
        // weight falling smoothly to nothing one site spacing away. Only a
        // cell's own site and its neighbours come that close, so the blend is
        // the same either side of a border between two sites.
        let angle = |a: Vec3, b: Vec3| {
            (a[0] * b[0] + a[1] * b[1] + a[2] * b[2])
                .clamp(-1.0, 1.0)
                .acos()
        };
        let (mut reach, mut pairs) = (0.0f64, 0usize);
        for (s, nbrs) in site_nbrs.iter().enumerate() {
            for &o in nbrs {
                reach += angle(sites[s], sites[o as usize]);
                pairs += 1;
            }
        }
        let reach = if pairs > 0 {
            reach / pairs as f64
        } else {
            std::f64::consts::PI
        };
        let mut blend_start = Vec::with_capacity(n + 1);
        let mut blend = Vec::with_capacity(n * 4);
        blend_start.push(0u32);
        for cell in 0..n {
            let c = [
                mesh.centres[cell * 3] as f64,
                mesh.centres[cell * 3 + 1] as f64,
                mesh.centres[cell * 3 + 2] as f64,
            ];
            let own = site_of[cell];
            let from = blend.len();
            let mut total = 0.0f64;
            for s in std::iter::once(own).chain(site_nbrs[own as usize].iter().copied()) {
                let r = angle(c, sites[s as usize]) / reach;
                if r < 1.0 {
                    let w = (1.0 - r * r).powi(2);
                    total += w;
                    blend.push((s, w as f32));
                }
            }
            if total > 0.0 {
                for b in &mut blend[from..] {
                    b.1 = (b.1 as f64 / total) as f32;
                }
            } else {
                blend.push((own, 1.0));
            }
            blend_start.push(blend.len() as u32);
        }

        let mut mantle = MantleFlow {
            site_of,
            sites,
            site_area,
            site_start,
            site_cells,
            site_nbrs,
            blend_start,
            blend,
            sink: vec![0.0; count],
            source: vec![0.0; count],
            flux: vec![0.0; count],
            wind: vec![[0.0; 3]; count],
            upper_temp: vec![0.0; count],
            cell_temp: vec![0.0; n],
            transit: vec![0.0; count],
            lower_slab: vec![0.0; count],
            cell_pile: vec![0.0; n],
            plume_total: vec![0.0; count],
            plume_peak: 1.0,
        };
        mantle.weigh_plumes(&Tuning::default());
        mantle
    }

    /// Forget every downwelling, anomaly and pile, as for a world rebuilt
    /// from scratch.
    pub fn reset(&mut self) {
        self.sink.fill(0.0);
        self.source.fill(0.0);
        self.wind.fill([0.0; 3]);
        self.upper_temp.fill(0.0);
        self.cell_temp.fill(0.0);
        self.transit.fill(0.0);
        self.lower_slab.fill(0.0);
        self.cell_pile.fill(0.0);
        self.weigh_plumes(&Tuning::default());
    }

    /// The upper mantle's temperature beneath cell `i` (K) above or below the
    /// planet's mantle temperature, blended from the sites around it so the
    /// coarse grid does not show.
    pub fn upper_temp_at(&self, i: usize) -> f64 {
        let (from, to) = (
            self.blend_start[i] as usize,
            self.blend_start[i + 1] as usize,
        );
        self.blend[from..to]
            .iter()
            .map(|&(s, w)| w as f64 * self.upper_temp[s as usize])
            .sum()
    }

    /// `upper_temp_at` for every cell (K), as of the last update.
    pub fn cell_temps(&self) -> &[f32] {
        &self.cell_temp
    }

    /// Slab piled in the lower mantle beneath cell `i`, as a share of the
    /// area over it, blended from the sites around it like `upper_temp_at`.
    pub fn lower_slab_at(&self, i: usize) -> f64 {
        let (from, to) = (
            self.blend_start[i] as usize,
            self.blend_start[i + 1] as usize,
        );
        self.blend[from..to]
            .iter()
            .map(|&(s, w)| w as f64 * self.lower_slab[s as usize])
            .sum()
    }

    /// Remembered subduction beneath cell `i`, as a share of the area over it
    /// per Myr, blended from the sites around it like `upper_temp_at`.
    pub fn sink_at(&self, i: usize) -> f64 {
        let (from, to) = (
            self.blend_start[i] as usize,
            self.blend_start[i + 1] as usize,
        );
        self.blend[from..to]
            .iter()
            .map(|&(s, w)| w as f64 * self.sink[s as usize] / self.site_area[s as usize].max(1e-12))
            .sum()
    }

    /// Remembered subduction over the whole globe, as a share of its area per
    /// Myr: the rate at which the gentle upwelling everywhere else balances
    /// the downwellings.
    pub fn mean_sink(&self) -> f64 {
        let total: f64 = self.site_area.iter().sum();
        self.sink.iter().sum::<f64>() / total.max(1e-12)
    }

    /// `lower_slab_at` for every cell, as of the last update.
    pub fn cell_piles(&self) -> &[f32] {
        &self.cell_pile
    }

    /// Slab piled in the lower mantle over the whole globe, as a multiple of
    /// the globe's area.
    pub fn pile_mass(&self) -> f64 {
        let total: f64 = self.site_area.iter().sum();
        let piled: f64 = self.lower_slab.iter().zip(&self.site_area).map(|(x, a)| x * a).sum();
        piled / total.max(1e-12)
    }

    /// Largest site's plume weight over the mean by area: 1 when plumes are
    /// as likely to rise anywhere, higher the more the pile edges draw them.
    pub fn plume_peak(&self) -> f64 {
        self.plume_peak
    }

    /// Where a plume rises for `u` drawn evenly from [0, 1): a site chosen in
    /// proportion to its plume weight, and the centre of one of its cells,
    /// chosen by where `u` falls within that site's share.
    pub fn pick_plume_site(&self, mesh: &Mesh, u: f64) -> Vec3 {
        let total = self.plume_total.last().copied().unwrap_or(0.0);
        let at = u.clamp(0.0, 1.0) * total;
        let s = self
            .plume_total
            .partition_point(|&x| x <= at)
            .min(self.plume_total.len().saturating_sub(1));
        let before = if s > 0 { self.plume_total[s - 1] } else { 0.0 };
        let share = self.plume_total[s] - before;
        let within = if share > 0.0 {
            ((at - before) / share).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let cells = &self.site_cells[self.site_start[s] as usize..self.site_start[s + 1] as usize];
        if cells.is_empty() {
            return self.sites[s];
        }
        let cell = cells[((within * cells.len() as f64) as usize).min(cells.len() - 1)] as usize;
        [
            mesh.centres[cell * 3] as f64,
            mesh.centres[cell * 3 + 1] as f64,
            mesh.centres[cell * 3 + 2] as f64,
        ]
    }

    /// Coolest and hottest site, and the mean over the globe by area (K).
    pub fn anomaly_range(&self) -> (f64, f64, f64) {
        let (mut lo, mut hi, mut sum, mut area) = (f64::INFINITY, f64::NEG_INFINITY, 0.0, 0.0);
        for (&temp, &a) in self.upper_temp.iter().zip(&self.site_area) {
            lo = lo.min(temp);
            hi = hi.max(temp);
            sum += temp * a;
            area += a;
        }
        if area > 0.0 {
            (lo, hi, sum / area)
        } else {
            (0.0, 0.0, 0.0)
        }
    }

    /// Every mantle site and the downwelling remembered there.
    pub fn sinks(&self) -> (&[Vec3], &[f64]) {
        (&self.sites, &self.sink)
    }

    /// Put a downwelling of `rate` at site `site` and work out the flow.
    #[cfg(test)]
    pub fn set_sink_for_test(&mut self, site: usize, rate: f64) {
        self.sink[site] = rate;
        self.flow();
    }

    /// The mantle flow beneath cell `i`.
    #[inline]
    pub fn wind_at(&self, i: usize) -> Vec3 {
        self.wind[self.site_of[i] as usize]
    }

    /// Fold this step's subduction into the downwellings, warm and cool the
    /// upper mantle, and work out the flow they drive.
    pub fn update(
        &mut self,
        mesh: &Mesh,
        data: &CellData,
        boundaries: &BoundaryField,
        dt_myr: f64,
        t: &Tuning,
        o: &SimOptions,
    ) {
        let site_of = &self.site_of;
        let sinking = par::filter_map(mesh.arc_count(), |a| {
            if boundaries.arc_kind[a] != CONVERGENT {
                return None;
            }
            let (ca, cb) = (mesh.arc_cells[a * 2], mesh.arc_cells[a * 2 + 1]);
            if ca < 0 || cb < 0 {
                return None;
            }
            let closing = (boundaries.arc_closing[a] as f64).max(0.0);
            if closing <= 0.0 {
                return None;
            }
            let sub = subduction(mesh, data, ca as usize, cb as usize, t)?;
            Some((site_of[sub.down], closing * mesh.arc_length[a] as f64))
        });
        self.flux.fill(0.0);
        for (site, flux) in sinking {
            self.flux[site as usize] += flux;
        }
        let blend = if t.downwelling_memory > 0.0 {
            (dt_myr / t.downwelling_memory).min(1.0)
        } else {
            1.0
        };
        for (sink, &flux) in self.sink.iter_mut().zip(&self.flux) {
            *sink += (flux - *sink) * blend;
        }
        self.descend(dt_myr, t);
        self.warm(mesh, data, dt_myr, t, o);
        // Hot mantle rises as an upwelling that the surface flows out from:
        // what turns the mantle beneath a long-lived supercontinent from
        // downwelling to upwelling.
        self.source.fill(0.0);
        if t.upwelling_gain > 0.0 && t.upper_temp_max > 0.0 {
            let (temp, area) = (&self.upper_temp, &self.site_area);
            par::fill(&mut self.source, |s| {
                t.upwelling_gain * (temp[s] / t.upper_temp_max).max(0.0) * area[s]
            });
        }
        self.flow();
        self.weigh_plumes(t);
    }

    /// Carry the slab sunk at every site down into the lower mantle over
    /// `slab_transit_time`, and pile it there: spreading toward the
    /// neighbouring sites' piles and fading over `slab_pile_lifetime`. Slab
    /// sinking now, as for the upper mantle's temperature: the pile is its own
    /// memory. Then blend the piles out to every cell.
    fn descend(&mut self, dt_myr: f64, t: &Tuning) {
        let arrive = if t.slab_transit_time > 0.0 {
            1.0 - (-dt_myr / t.slab_transit_time).exp()
        } else {
            1.0
        };
        let keep = if t.slab_pile_lifetime > 0.0 {
            (-dt_myr / t.slab_pile_lifetime).exp()
        } else {
            0.0
        };
        let spread = (t.lower_diffusion * dt_myr).clamp(0.0, 1.0);
        let mut landed = vec![0.0f64; self.sites.len()];
        for ((transit, landed), (&flux, &area)) in self
            .transit
            .iter_mut()
            .zip(&mut landed)
            .zip(self.flux.iter().zip(&self.site_area))
        {
            *transit += flux / area.max(1e-12) * dt_myr;
            *landed = *transit * arrive;
            *transit -= *landed;
        }
        let mut next = vec![0.0f64; self.sites.len()];
        let (prev, nbrs) = (&self.lower_slab, &self.site_nbrs);
        par::fill(&mut next, |s| {
            let own = prev[s];
            let around = if nbrs[s].is_empty() {
                own
            } else {
                nbrs[s].iter().map(|&m| prev[m as usize]).sum::<f64>() / nbrs[s].len() as f64
            };
            (own + (around - own) * spread + landed[s]) * keep
        });
        self.lower_slab = next;
        let mut cell_pile = std::mem::take(&mut self.cell_pile);
        let this = &*self;
        par::fill(&mut cell_pile, |i| this.lower_slab_at(i) as f32);
        self.cell_pile = cell_pile;
    }

    /// Weigh every site for the plumes that rise from it: by how far its
    /// neighbours' piles outweigh its own, so most at a pile's edge and none
    /// at its heart or far out in open mantle, more again where the upper
    /// mantle above runs hot, and with `plume_floor` of the mean spread over
    /// the globe so plumes still rise where there are no piles.
    fn weigh_plumes(&mut self, t: &Tuning) {
        let count = self.sites.len();
        let mut weight = vec![0.0f64; count];
        let (pile, nbrs, temp, area) = (&self.lower_slab, &self.site_nbrs, &self.upper_temp, &self.site_area);
        par::fill(&mut weight, |s| {
            if nbrs[s].is_empty() {
                return 0.0;
            }
            let around = nbrs[s].iter().map(|&m| pile[m as usize]).sum::<f64>() / nbrs[s].len() as f64;
            let warm = if t.upper_temp_max > 0.0 {
                1.0 + temp[s].max(0.0) / t.upper_temp_max
            } else {
                1.0
            };
            (around - pile[s]).max(0.0) * warm * area[s]
        });
        let globe: f64 = area.iter().sum::<f64>().max(1e-12);
        let edges: f64 = weight.iter().sum();
        // With no edge anywhere, plumes rise anywhere alike.
        let floor = if edges > 0.0 {
            t.plume_floor.max(0.0) * edges / globe
        } else {
            1.0
        };
        let mut running = 0.0;
        let mut peak = 0.0f64;
        for (s, w) in weight.iter().enumerate() {
            let w = w + floor * area[s];
            peak = peak.max(w / area[s].max(1e-12));
            running += w;
            self.plume_total[s] = running;
        }
        self.plume_peak = if running > 0.0 {
            peak / (running / globe)
        } else {
            1.0
        };
    }

    /// Step the upper mantle's temperature at every site: cooled by the slab
    /// sinking there and a little by the pile beneath it, warmed by the
    /// continent over it, spread toward its neighbours' and relaxed back
    /// toward the planet's mantle temperature. Then blend it out to every
    /// cell.
    fn warm(&mut self, mesh: &Mesh, data: &CellData, dt_myr: f64, t: &Tuning, o: &SimOptions) {
        let count = self.sites.len();
        let mut share = vec![0.0f64; count];
        if o.supercontinent_insulation {
            let (start, cells, area) = (&self.site_start, &self.site_cells, &self.site_area);
            par::fill(&mut share, |s| {
                let covered: f64 = cells[start[s] as usize..start[s + 1] as usize]
                    .iter()
                    .map(|&c| c as usize)
                    .filter(|&i| data.crust_type[i] == CONTINENTAL)
                    .map(|i| mesh.area(i))
                    .sum();
                covered / area[s].max(1e-12)
            });
        }
        let spread = (t.site_diffusion * dt_myr).clamp(0.0, 1.0);
        let keep = if t.upper_relax_time > 0.0 {
            1.0 / (1.0 + dt_myr / t.upper_relax_time)
        } else {
            0.0
        };
        let limit = t.upper_temp_max.max(0.0);
        let mut next = vec![0.0f64; count];
        // Slab sinking now, not the downwelling remembered after it: the
        // anomaly keeps its own memory, and mantle under a continent gathered
        // over an old trench must warm once the trench has gone quiet.
        let (prev, nbrs, flux, area, pile) = (
            &self.upper_temp,
            &self.site_nbrs,
            &self.flux,
            &self.site_area,
            &self.lower_slab,
        );
        par::fill(&mut next, |s| {
            let own = prev[s];
            let around = if nbrs[s].is_empty() {
                own
            } else {
                nbrs[s].iter().map(|&m| prev[m as usize]).sum::<f64>() / nbrs[s].len() as f64
            };
            // Slab sunk, as a share of the site's area per Myr.
            let sunk = flux[s] / area[s].max(1e-12);
            let forcing = t.site_insulation * share[s] - t.slab_cooling * sunk - t.pile_cooling * pile[s];
            (own + (around - own) * spread + forcing * dt_myr) * keep
        });
        // The planet's mantle temperature is the mean (see `heat.rs`), so the
        // anomalies only say where the mantle is hotter or cooler than it: cold
        // slab under one site is mantle displaced up everywhere else.
        let total: f64 = area.iter().sum();
        let mean = next.iter().zip(area).map(|(x, a)| x * a).sum::<f64>() / total.max(1e-12);
        for temp in &mut next {
            *temp = (*temp - mean).max(-limit).min(limit);
        }
        self.upper_temp = next;
        let mut cell_temp = std::mem::take(&mut self.cell_temp);
        let this = &*self;
        par::fill(&mut cell_temp, |i| this.upper_temp_at(i) as f32);
        self.cell_temp = cell_temp;
    }

    /// The flow at every site from the downwellings as they stand.
    fn flow(&mut self) {
        let count = self.sites.len();
        let spacing = (4.0 * std::f64::consts::PI / count as f64).sqrt();
        let near = (NEAR_SITES * spacing).min(std::f64::consts::PI);
        let floor = near.sin().powi(2);
        let per_sink = 1.0 / (4.0 * std::f64::consts::PI);
        let (sites, sink, source) = (&self.sites, &self.sink, &self.source);
        par::fill_costly(&mut self.wind, |i| {
            let p = sites[i];
            let mut u = [0.0f64; 3];
            for (j, (&down, &up)) in sink.iter().zip(source).enumerate() {
                // Net sink: negative over an upwelling, which flows outward.
                let q = down - up;
                if q == 0.0 || j == i {
                    continue;
                }
                let s = sites[j];
                let cos = p[0] * s[0] + p[1] * s[1] + p[2] * s[2];
                // Toward the sink along the surface, unnormalised: its length
                // is sin d.
                let toward = [s[0] - cos * p[0], s[1] - cos * p[1], s[2] - cos * p[2]];
                let sin2 = (1.0 - cos * cos).max(floor);
                // cot(d/2) = (1 + cos d) / sin d, along a unit vector.
                let w = q * per_sink * (1.0 + cos) / sin2;
                u[0] += w * toward[0];
                u[1] += w * toward[1];
                u[2] += w * toward[2];
            }
            u
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{dot, len3};

    #[test]
    fn the_mantle_flows_toward_a_downwelling_and_fades_with_distance() {
        let mesh = Mesh::icosphere(4);
        let mut mantle = MantleFlow::new(&mesh);
        mantle.sink[0] = 1.0;
        mantle.flow();
        let sink = mantle.sites[0];
        let mut by_distance: Vec<(f64, f64)> = Vec::new();
        for (i, &p) in mantle.sites.iter().enumerate().skip(1) {
            let u = mantle.wind[i];
            let speed = len3(u[0], u[1], u[2]);
            let cos = dot(p, sink);
            let toward = [
                sink[0] - cos * p[0],
                sink[1] - cos * p[1],
                sink[2] - cos * p[2],
            ];
            if cos > -0.99 {
                assert!(dot(u, toward) > 0.0, "site {i} flows away from the sink");
                assert!(
                    dot(u, p).abs() < 1e-5 * speed,
                    "site {i} flows off the surface"
                );
            }
            by_distance.push((cos.acos(), speed));
        }
        by_distance.sort_by(|a, b| a.0.total_cmp(&b.0));
        let near = by_distance[by_distance.len() / 10].1;
        let far = by_distance[by_distance.len() * 9 / 10].1;
        assert!(
            near > 4.0 * far,
            "flow barely fades: {near} near, {far} far"
        );
        let antipode = by_distance.last().unwrap();
        assert!(
            antipode.1 < 0.05 * near,
            "flow at the antipode: {}",
            antipode.1
        );
    }

    /// Step only the upper mantle's temperature, `myr` Myr at the default tick.
    fn warm_for(mantle: &mut MantleFlow, mesh: &Mesh, data: &CellData, myr: f64) {
        let t = Tuning::default();
        let o = SimOptions::default();
        for _ in 0..(myr / t.dt_myr) as usize {
            mantle.warm(mesh, data, t.dt_myr, &t, &o);
        }
    }

    #[test]
    fn a_steady_sink_cools_its_site_and_its_neighbours() {
        let mesh = Mesh::icosphere(5);
        let data = CellData::new(mesh.cell_count);
        let mut mantle = MantleFlow::new(&mesh);
        // A trench sinking a fifth of the site's area every 10 Myr.
        mantle.flux[0] = 0.02 * mantle.site_area[0];
        warm_for(&mut mantle, &mesh, &data, 300.0);
        let own = mantle.upper_temp[0];
        assert!(own < -20.0, "sink site at {own:.1} K");
        for &s in &mantle.site_nbrs[0] {
            let near = mantle.upper_temp[s as usize];
            assert!(
                near < 0.1 * own && near > own,
                "neighbour {s} at {near:.1} K"
            );
        }
        let far = mantle
            .sites
            .iter()
            .position(|p| dot(*p, mantle.sites[0]) < -0.9)
            .unwrap();
        assert!(
            mantle.upper_temp[far].abs() < 1.0,
            "far side at {:.2} K",
            mantle.upper_temp[far]
        );
    }

    #[test]
    fn a_large_still_continent_warms_the_sites_beneath_it() {
        let mesh = Mesh::icosphere(5);
        let mut data = CellData::new(mesh.cell_count);
        for i in 0..mesh.cell_count {
            if mesh.centres[i * 3 + 2] > 0.3 {
                data.crust_type[i] = CONTINENTAL;
            }
        }
        let mut mantle = MantleFlow::new(&mesh);
        warm_for(&mut mantle, &mesh, &data, 400.0);
        let (mut interior, mut ocean) = (f64::INFINITY, f64::NEG_INFINITY);
        for (s, p) in mantle.sites.iter().enumerate() {
            if p[2] > 0.7 {
                interior = interior.min(mantle.upper_temp[s]);
            } else if p[2] < -0.3 {
                ocean = ocean.max(mantle.upper_temp[s]);
            }
        }
        assert!(
            interior > 80.0,
            "coolest site under the continent at {interior:.1} K"
        );
        assert!(
            ocean < 0.1 * interior,
            "warmest open-ocean site at {ocean:.1} K"
        );
        // Off, the continent holds nothing in.
        let mut off = MantleFlow::new(&mesh);
        let t = Tuning::default();
        let o = SimOptions {
            supercontinent_insulation: false,
            ..SimOptions::default()
        };
        off.warm(&mesh, &data, t.dt_myr, &t, &o);
        assert!(off.upper_temp.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn an_isolated_anomaly_spreads_and_fades() {
        let mesh = Mesh::icosphere(4);
        let data = CellData::new(mesh.cell_count);
        let mut mantle = MantleFlow::new(&mesh);
        mantle.upper_temp[0] = 200.0;
        let peak = |m: &MantleFlow| m.upper_temp.iter().fold(0.0f64, |p, x| p.max(x.abs()));
        warm_for(&mut mantle, &mesh, &data, 20.0);
        let own = mantle.upper_temp[0];
        assert!(own < 200.0 && own > 0.0, "site at {own:.1} K");
        for &s in &mantle.site_nbrs[0] {
            assert!(
                mantle.upper_temp[s as usize] > 0.0,
                "neighbour {s} never warmed"
            );
        }
        warm_for(&mut mantle, &mesh, &data, 500.0);
        let left = peak(&mantle);
        assert!(left < 0.05 * 200.0, "largest anomaly left {left:.1} K");
        let mean: f64 = mantle
            .upper_temp
            .iter()
            .zip(&mantle.site_area)
            .map(|(x, a)| x * a)
            .sum();
        assert!(mean.abs() < 1e-9, "the anomalies sum to {mean}");
    }

    #[test]
    fn the_blended_temperature_has_no_steps_at_site_borders() {
        let mesh = Mesh::icosphere(6);
        let mut mantle = MantleFlow::new(&mesh);
        let peak = 100.0;
        // A pentagonal site, and others spread over the grid.
        for site in [0usize, 100, 333, 641] {
            mantle.upper_temp.fill(0.0);
            mantle.upper_temp[site] = peak;
            let at: Vec<f64> = (0..mesh.cell_count)
                .map(|i| mantle.upper_temp_at(i))
                .collect();
            assert!(
                at[site] > 0.8 * peak,
                "site {site} itself at {:.2} K",
                at[site]
            );
            let mut worst = 0.0f64;
            for i in 0..mesh.cell_count {
                for k in mesh.nbrs(i) {
                    worst = worst.max((at[i] - at[mesh.nbr[k] as usize]).abs());
                }
            }
            // Read straight off the sites, the border would step the whole
            // peak.
            assert!(
                worst < 0.25 * peak,
                "site {site}: step of {worst:.1} K between neighbouring cells"
            );
        }
    }

    /// Step only the lower mantle, `myr` Myr at the default tick.
    fn descend_for(mantle: &mut MantleFlow, myr: f64) {
        let t = Tuning::default();
        for _ in 0..(myr / t.dt_myr).round() as usize {
            mantle.descend(t.dt_myr, &t);
        }
    }

    /// Slab piled over the whole lower mantle, in multiples of site 0's area.
    fn piled(mantle: &MantleFlow) -> f64 {
        mantle.pile_mass() * mantle.site_area.iter().sum::<f64>() / mantle.site_area[0]
    }

    #[test]
    fn sunk_slab_reaches_the_lower_mantle_after_its_transit() {
        let mesh = Mesh::icosphere(4);
        let mut mantle = MantleFlow::new(&mesh);
        let rate = 0.02;
        mantle.flux[0] = rate * mantle.site_area[0];
        let transit = Tuning::default().slab_transit_time;
        descend_for(&mut mantle, 0.5 * transit);
        let early = piled(&mantle);
        descend_for(&mut mantle, 0.5 * transit);
        let at_transit = piled(&mantle);
        assert!(early > 0.0, "nothing landed in the first {:.0} Myr", 0.5 * transit);
        // Landing at once, it would have piled `rate` per Myr from the start.
        assert!(
            at_transit < 0.5 * rate * transit,
            "{at_transit:.2} piled by {transit:.0} Myr of {:.2} sunk",
            rate * transit
        );
        // Still in transit, the flow down gathers pace.
        assert!(early < 0.4 * at_transit, "{early:.3} at half the transit, {at_transit:.3} at it");
        descend_for(&mut mantle, 3.0 * transit);
        assert!(piled(&mantle) > 4.0 * at_transit, "the pile stopped growing");
        let centre = mantle.lower_slab[0];
        for &s in &mantle.site_nbrs[0] {
            let near = mantle.lower_slab[s as usize];
            assert!(near > 0.0 && near < centre, "neighbour {s} pile {near:.3} of {centre:.3}");
        }
    }

    #[test]
    fn a_pile_fades_once_its_slab_stops_sinking() {
        let mesh = Mesh::icosphere(4);
        let mut mantle = MantleFlow::new(&mesh);
        let t = Tuning::default();
        mantle.flux[0] = 0.02 * mantle.site_area[0];
        descend_for(&mut mantle, 400.0);
        mantle.flux[0] = 0.0;
        descend_for(&mut mantle, 3.0 * t.slab_transit_time);
        let before = piled(&mantle);
        descend_for(&mut mantle, t.slab_pile_lifetime);
        let ratio = piled(&mantle) / before;
        assert!(
            (0.25..0.5).contains(&ratio),
            "{:.0}% of the pile left after {:.0} Myr",
            ratio * 100.0,
            t.slab_pile_lifetime
        );
    }

    /// Plume weight of each site, per unit of its area.
    fn weights(mantle: &MantleFlow) -> Vec<f64> {
        (0..mantle.sites.len())
            .map(|s| {
                let before = if s > 0 { mantle.plume_total[s - 1] } else { 0.0 };
                (mantle.plume_total[s] - before) / mantle.site_area[s]
            })
            .collect()
    }

    #[test]
    fn plumes_rise_at_the_edge_of_a_pile() {
        let mesh = Mesh::icosphere(4);
        let mut mantle = MantleFlow::new(&mesh);
        let t = Tuning::default();
        let empty = weights(&mantle);
        let mean = empty.iter().sum::<f64>() / empty.len() as f64;
        assert!(empty.iter().all(|w| (w - mean).abs() < 1e-9 * mean), "an empty mantle favours somewhere");
        assert!((mantle.plume_peak() - 1.0).abs() < 1e-9);

        let centre = 100usize;
        mantle.lower_slab[centre] = 10.0;
        mantle.weigh_plumes(&t);
        let w = weights(&mantle);
        let ring = &mantle.site_nbrs[centre];
        let far = mantle
            .sites
            .iter()
            .position(|p| dot(*p, mantle.sites[centre]) < -0.9)
            .unwrap();
        for &s in ring {
            assert!(w[s as usize] > 5.0 * w[far], "edge site {s} at {:.3}, far at {:.3}", w[s as usize], w[far]);
        }
        assert!(w[centre] <= w[far] * (1.0 + 1e-9), "the pile's heart at {:.3}", w[centre]);
        assert!(mantle.plume_peak() > 5.0, "peak only {:.1}x the mean", mantle.plume_peak());
        // Warm upper mantle over an edge draws more plumes up through it.
        let edge = ring[0] as usize;
        mantle.upper_temp[edge] = t.upper_temp_max;
        mantle.weigh_plumes(&t);
        assert!(weights(&mantle)[edge] > 1.5 * w[edge]);
    }

    #[test]
    fn plumes_are_picked_by_weight_and_the_same_way_every_time() {
        let mesh = Mesh::icosphere(4);
        let mut mantle = MantleFlow::new(&mesh);
        let centre = 100usize;
        mantle.lower_slab[centre] = 10.0;
        mantle.weigh_plumes(&Tuning::default());
        let picks = |seed: &str| {
            let mut rng = crate::rng::Rng::new(seed);
            (0..200)
                .map(|_| mantle.pick_plume_site(&mesh, rng.next_f64()))
                .collect::<Vec<Vec3>>()
        };
        let first = picks("plumes");
        assert_eq!(first, picks("plumes"));
        assert_ne!(first, picks("other"));
        let ring = &mantle.site_nbrs[centre];
        let at_edge = first
            .iter()
            .filter(|p| {
                let cell = (0..mesh.cell_count)
                    .find(|&i| {
                        (0..3).all(|k| mesh.centres[i * 3 + k] as f64 == p[k])
                    })
                    .expect("a pick off every cell centre");
                ring.contains(&mantle.site_of[cell])
            })
            .count();
        assert!(at_edge > 160, "{at_edge} of 200 plumes at the pile's edge");
    }

    #[test]
    fn the_surface_rises_over_hot_mantle_and_sinks_over_a_downwelling() {
        use crate::elevation::dynamic_topography;
        let mesh = Mesh::icosphere(5);
        let mut mantle = MantleFlow::new(&mesh);
        let t = Tuning::default();
        let (hot, cold) = (100usize, 400usize);
        mantle.upper_temp[hot] = 150.0;
        mantle.sink[cold] = 0.03 * mantle.site_area[cold];
        let balance = mantle.mean_sink();
        let topo: Vec<f64> = (0..mesh.cell_count)
            .map(|i| dynamic_topography(&t, mantle.upper_temp_at(i), mantle.sink_at(i) - balance))
            .collect();
        let over = |s: usize| {
            let cells = &mantle.site_cells[mantle.site_start[s] as usize..mantle.site_start[s + 1] as usize];
            cells.iter().map(|&c| topo[c as usize]).sum::<f64>() / cells.len() as f64
        };
        let far = mantle
            .sites
            .iter()
            .position(|p| dot(*p, mantle.sites[hot]) < -0.5 && dot(*p, mantle.sites[cold]) < -0.5)
            .unwrap();
        assert!(over(hot) > 0.5, "hot site raised only {:.2} km", over(hot));
        assert!(over(cold) < -0.2, "sink site lowered only {:.2} km", over(cold));
        assert!(over(far).abs() < 0.01, "open mantle moved {:.3} km", over(far));
        // The rest of the globe rises a little to balance the downwelling.
        assert!(over(far) > 0.0);
        let limit = t.dyn_topo_max;
        assert!(topo.iter().all(|x| x.abs() <= limit));
        mantle.upper_temp[hot] = 1e4;
        assert_eq!(
            dynamic_topography(&t, mantle.upper_temp_at(hot), mantle.sink_at(hot) - balance),
            limit
        );
    }

    #[test]
    fn every_cell_lies_over_a_nearby_site() {
        let mesh = Mesh::icosphere(5);
        let mantle = MantleFlow::new(&mesh);
        let spacing = (4.0 * std::f64::consts::PI / mantle.sites.len() as f64).sqrt();
        for i in 0..mesh.cell_count {
            let c = [
                mesh.centres[i * 3] as f64,
                mesh.centres[i * 3 + 1] as f64,
                mesh.centres[i * 3 + 2] as f64,
            ];
            let s = mantle.sites[mantle.site_of[i] as usize];
            assert!(
                dot(c, s).clamp(-1.0, 1.0).acos() < spacing,
                "cell {i} far from its site"
            );
        }
    }
}
