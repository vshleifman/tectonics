//! Plate assignment and persistent plate identity, mirroring
//! `PlateRegistry.ts`: a flood fill that never crosses a cracked arc, and a
//! relabel that maps its raw labels onto stable ids by maximum overlap.

use crate::mesh::Mesh;
use indexmap::IndexMap;
use std::collections::HashMap;

/// Flood-fill connected components, never crossing a cracked arc. Writes raw
/// labels in `[0, count)` into `out` and returns `count`.
pub fn assign_plates(mesh: &Mesh, cracked: &[u8], out: &mut [i32]) -> usize {
    out.fill(-1);
    let mut stack: Vec<usize> = Vec::new();
    let mut count = 0;
    for s in 0..mesh.cell_count {
        if out[s] != -1 {
            continue;
        }
        out[s] = count as i32;
        stack.clear();
        stack.push(s);
        while let Some(cell) = stack.pop() {
            for k in mesh.nbrs(cell) {
                let arc = mesh.nbr_arc[k];
                if arc >= 0 && cracked.get(arc as usize).copied().unwrap_or(0) != 0 {
                    continue;
                }
                let j = mesh.nbr[k] as usize;
                if out[j] == -1 {
                    out[j] = count as i32;
                    stack.push(j);
                }
            }
        }
        count += 1;
    }
    count
}

#[derive(Debug, Default)]
pub struct RelabelReport {
    pub kept: Vec<i32>,
    pub new_ids: Vec<i32>,
    pub retired: Vec<i32>,
    /// Parent stable id for each entry of `new_ids`, or -1.
    pub parent_of: Vec<i32>,
}

#[derive(Default)]
pub struct Registry {
    next_id: i32,
    has_prev: bool,
    /// Stable ids currently carrying cells, in insertion order.
    live: Vec<i32>,
}

impl Registry {
    /// Take the next stable id, for a plate the simulation itself creates.
    pub fn allocate(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Map raw labels onto stable ids, writing them over the previous
    /// assignment held in `out`.
    pub fn relabel(&mut self, raw: &[i32], raw_count: usize, out: &mut [i32]) -> RelabelReport {
        // 1. Tally previous-id overlap per raw label.
        let mut overlaps: Vec<HashMap<i32, u32>> = vec![HashMap::new(); raw_count];
        if self.has_prev {
            for (i, &r) in raw.iter().enumerate() {
                if r < 0 || out[i] < 0 {
                    continue;
                }
                *overlaps[r as usize].entry(out[i]).or_insert(0) += 1;
            }
        }

        // 2. Each raw label claims the previous id it overlaps most; ties go to
        //    the older (smaller) id.
        let mut claim_prev = vec![-1i32; raw_count];
        let mut claim_count = vec![0u32; raw_count];
        for r in 0..raw_count {
            let mut best_id = -1;
            let mut best_count = 0;
            for (&prev, &count) in &overlaps[r] {
                if count > best_count || (count == best_count && (best_id < 0 || prev < best_id)) {
                    best_count = count;
                    best_id = prev;
                }
            }
            claim_prev[r] = best_id;
            claim_count[r] = best_count;
        }

        // 3. Conflicts (splits): the raw label with the largest overlap keeps
        //    the previous id.
        let mut winner: IndexMap<i32, usize> = IndexMap::new();
        for r in 0..raw_count {
            let prev = claim_prev[r];
            if prev < 0 {
                continue;
            }
            match winner.get(&prev).copied() {
                None => {
                    winner.insert(prev, r);
                }
                Some(cur) => {
                    if claim_count[r] > claim_count[cur]
                        || (claim_count[r] == claim_count[cur] && r < cur)
                    {
                        winner.insert(prev, r);
                    }
                }
            }
        }

        // 4. Winners inherit their id; everyone else gets a fresh one.
        let mut stable = vec![-1i32; raw_count];
        let mut report = RelabelReport::default();
        for (&prev, &r) in &winner {
            stable[r] = prev;
            report.kept.push(prev);
        }
        for r in 0..raw_count {
            if stable[r] >= 0 {
                continue;
            }
            let id = self.allocate();
            stable[r] = id;
            report.new_ids.push(id);
            report.parent_of.push(claim_prev[r]);
        }

        // 5. Write stable ids back over the previous assignment.
        for (i, &r) in raw.iter().enumerate() {
            out[i] = if r < 0 { -1 } else { stable[r as usize] };
        }

        // 6. Anything live before but not kept has been merged away.
        for &id in &self.live {
            if !report.kept.contains(&id) {
                report.retired.push(id);
            }
        }
        self.live = report.kept.iter().chain(&report.new_ids).copied().collect();
        self.has_prev = true;
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_keeps_the_larger_piece_identity() {
        let mut reg = Registry::default();
        let mut ids = vec![0i32; 6];
        let report = reg.relabel(&[0, 0, 0, 0, 0, 0], 1, &mut ids);
        assert_eq!(report.new_ids, vec![0]);
        let report = reg.relabel(&[0, 0, 0, 0, 1, 1], 2, &mut ids);
        assert_eq!(report.kept, vec![0]);
        assert_eq!(report.new_ids, vec![1]);
        assert_eq!(report.parent_of, vec![0]);
        assert_eq!(ids, vec![0, 0, 0, 0, 1, 1]);
    }
}
