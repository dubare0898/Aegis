use aegis_schema::{Affiliation, Track, TruthEntity, Vec3};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TruthScore {
    pub matched: usize,
    pub false_tracks: usize,
    pub missed_truth: usize,
    pub position_rmse_m: f64,
    pub track_purity: f64,
    pub track_completeness: f64,
    pub false_track_rate: f64,
    pub missed_truth_rate: f64,
}

/// Greedy nearest-neighbor association between hostile truth and non-friendly tracks.
pub fn score_tracks(truth: &[TruthEntity], tracks: &[Track], gate_m: f64) -> TruthScore {
    // Score air hostiles only (exclude friendlies and static fiber GCS/spool sites).
    let hostiles: Vec<&TruthEntity> = truth
        .iter()
        .filter(|e| e.affiliation == Affiliation::Hostile)
        .collect();
    let candidates: Vec<&Track> = tracks
        .iter()
        .filter(|tr| tr.affiliation != Affiliation::Friendly)
        .collect();

    let mut used_tracks = vec![false; candidates.len()];
    let mut used_truth = vec![false; hostiles.len()];
    let mut pairs: Vec<(usize, usize, f64)> = Vec::new();

    for (ti, t) in hostiles.iter().enumerate() {
        for (ci, c) in candidates.iter().enumerate() {
            let d = t.position.distance(&c.position);
            if d <= gate_m {
                pairs.push((ti, ci, d));
            }
        }
    }
    pairs.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    let mut matched = 0usize;
    let mut sum_sq = 0.0;
    for (ti, ci, d) in pairs {
        if used_truth[ti] || used_tracks[ci] {
            continue;
        }
        used_truth[ti] = true;
        used_tracks[ci] = true;
        matched += 1;
        sum_sq += d * d;
    }

    let false_tracks = used_tracks.iter().filter(|u| !**u).count();
    let missed_truth = used_truth.iter().filter(|u| !**u).count();
    let cand_n = candidates.len().max(1) as f64;
    let truth_n = hostiles.len().max(1) as f64;

    TruthScore {
        matched,
        false_tracks,
        missed_truth,
        position_rmse_m: if matched > 0 {
            (sum_sq / matched as f64).sqrt()
        } else {
            0.0
        },
        track_purity: matched as f64 / cand_n,
        track_completeness: matched as f64 / truth_n,
        false_track_rate: false_tracks as f64 / cand_n,
        missed_truth_rate: missed_truth as f64 / truth_n,
    }
}

#[allow(dead_code)]
pub fn centroid(points: &[Vec3]) -> Vec3 {
    if points.is_empty() {
        return Vec3::zero();
    }
    let n = points.len() as f64;
    Vec3::new(
        points.iter().map(|p| p.x).sum::<f64>() / n,
        points.iter().map(|p| p.y).sum::<f64>() / n,
        points.iter().map(|p| p.z).sum::<f64>() / n,
    )
}

#[derive(Debug, Default)]
pub struct SmoothnessAccum {
    prev: HashMap<Uuid, (f64, Vec3)>,
    heading_deltas: Vec<f64>,
    accels: Vec<f64>,
}

impl SmoothnessAccum {
    pub fn observe(&mut self, truth: &[TruthEntity], dt: f64) {
        if dt <= 1e-9 {
            return;
        }
        for e in truth {
            if e.affiliation != Affiliation::Hostile || e.neutralized {
                continue;
            }
            // Skip near-static ground/spool-like hostiles if any.
            let speed = e.velocity.magnitude_xy();
            if speed < 0.5 {
                continue;
            }
            let heading = e.velocity.y.atan2(e.velocity.x);
            if let Some((prev_h, prev_v)) = self.prev.get(&e.id) {
                let mut dh = heading - prev_h;
                while dh > std::f64::consts::PI {
                    dh -= std::f64::consts::TAU;
                }
                while dh < -std::f64::consts::PI {
                    dh += std::f64::consts::TAU;
                }
                self.heading_deltas.push(dh.abs());
                let dvx = e.velocity.x - prev_v.x;
                let dvy = e.velocity.y - prev_v.y;
                let dvz = e.velocity.z - prev_v.z;
                let accel = (dvx * dvx + dvy * dvy + dvz * dvz).sqrt() / dt;
                self.accels.push(accel);
            }
            self.prev.insert(e.id, (heading, e.velocity));
        }
    }

    pub fn max_heading_delta_rad(&self) -> f64 {
        self.heading_deltas.iter().cloned().fold(0.0, f64::max)
    }

    pub fn p95_accel_mps2(&self) -> f64 {
        if self.accels.is_empty() {
            return 0.0;
        }
        let mut sorted = self.accels.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((sorted.len() as f64) * 0.95).floor() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    pub fn violation_count(&self) -> usize {
        let h = self
            .heading_deltas
            .iter()
            .filter(|d| **d > MAX_HEADING_DELTA_RAD)
            .count();
        let a = self.accels.iter().filter(|v| **v > MAX_ACCEL_MPS2).count();
        h + a
    }

    pub fn kinematics_ok(&self) -> bool {
        self.max_heading_delta_rad() <= MAX_HEADING_DELTA_RAD
            && self.p95_accel_mps2() <= MAX_P95_ACCEL_MPS2
    }
}

/// Hard kinematic invariant ceilings (swarm steering must stay under these).
pub const MAX_HEADING_DELTA_RAD: f64 = 0.55;
pub const MAX_ACCEL_MPS2: f64 = 55.0;
pub const MAX_P95_ACCEL_MPS2: f64 = 45.0;
/// Soak / batch: fused track count must stay below this.
pub const MAX_TRACK_COUNT: usize = 400;

/// No NaN/Inf on hostile air truth kinematics.
pub fn truth_kinematics_finite(truth: &[TruthEntity]) -> bool {
    truth.iter().all(|e| {
        if e.affiliation != Affiliation::Hostile {
            return true;
        }
        e.position.x.is_finite()
            && e.position.y.is_finite()
            && e.position.z.is_finite()
            && e.velocity.x.is_finite()
            && e.velocity.y.is_finite()
            && e.velocity.z.is_finite()
    })
}
