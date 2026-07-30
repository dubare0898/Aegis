use aegis_schema::{
    Affiliation, Detection, IdGen, IdentityBasis, SensorKind, Track, TrackClass, TrackStatus, Vec3,
};
use std::collections::HashSet;
use uuid::Uuid;

/// Stream id for track IDs (sim uses stream 1).
pub const FUSION_ID_STREAM: u64 = 2;

#[derive(Debug)]
struct InternalTrack {
    id: Uuid,
    t: f64,
    x: f64,
    y: f64,
    z: f64,
    vx: f64,
    vy: f64,
    vz: f64,
    p: f64,
    class_hypothesis: TrackClass,
    class_confidence: f64,
    affiliation: Affiliation,
    sensors: HashSet<SensorKind>,
    hit_count: u32,
    coast_ticks: u32,
    born_t: f64,
    rf_dark_hits: u32,
    rf_hits: u32,
}

#[derive(Debug)]
pub struct FusionEngine {
    tracks: Vec<InternalTrack>,
    gate_m: f64,
    coast_limit: u32,
    ids: IdGen,
    seed: u64,
}

impl FusionEngine {
    pub fn new(seed: u64) -> Self {
        Self {
            tracks: Vec::new(),
            gate_m: 180.0,
            coast_limit: 45,
            ids: IdGen::new(seed, FUSION_ID_STREAM),
            seed,
        }
    }

    pub fn reset(&mut self) {
        self.tracks.clear();
        self.ids = IdGen::new(self.seed, FUSION_ID_STREAM);
    }

    pub fn reset_with_seed(&mut self, seed: u64) {
        self.seed = seed;
        self.reset();
    }

    pub fn process(&mut self, t: f64, dt: f64, detections: &[Detection]) -> Vec<Track> {
        for tr in &mut self.tracks {
            tr.x += tr.vx * dt;
            tr.y += tr.vy * dt;
            tr.z += tr.vz * dt;
            tr.p += 12.0 * dt;
            tr.coast_ticks += 1;
            tr.t = t;
        }

        // Global greedy: sort candidate pairs by cost so dense swarms don't
        // let early tracks monopolize nearby detections.
        let mut pairs: Vec<(usize, usize, f64)> = Vec::new();
        for (ti, tr) in self.tracks.iter().enumerate() {
            for (di, d) in detections.iter().enumerate() {
                if !associable(tr, d, self.gate_m) {
                    continue;
                }
                let cost = association_cost(tr, d);
                pairs.push((ti, di, cost));
            }
        }
        pairs.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
        let mut used_tracks = vec![false; self.tracks.len()];
        let mut used_dets = vec![false; detections.len()];
        let mut assignments: Vec<(usize, usize)> = Vec::new();
        for (ti, di, _) in pairs {
            if used_tracks[ti] || used_dets[di] {
                continue;
            }
            used_tracks[ti] = true;
            used_dets[di] = true;
            assignments.push((ti, di));
        }
        let unused: Vec<usize> = used_dets
            .iter()
            .enumerate()
            .filter_map(|(i, u)| if !*u { Some(i) } else { None })
            .collect();

        for (ti, di) in assignments {
            let d = &detections[di];
            let tr = &mut self.tracks[ti];
            let alpha = if d.sensor_kind == SensorKind::Acoustic {
                0.28
            } else {
                0.45
            };
            let innov_x = d.position.x - tr.x;
            let innov_y = d.position.y - tr.y;
            let innov_z = d.position.z - tr.z;
            tr.x += alpha * innov_x;
            tr.y += alpha * innov_y;
            tr.z += alpha * innov_z;
            // EMA blend — avoid hard-assigning latest detection velocity (smooths ETA/closing).
            if let Some(v) = d.velocity {
                const V_ALPHA: f64 = 0.18;
                tr.vx = (1.0 - V_ALPHA) * tr.vx + V_ALPHA * v.x;
                tr.vy = (1.0 - V_ALPHA) * tr.vy + V_ALPHA * v.y;
                tr.vz = (1.0 - V_ALPHA) * tr.vz + V_ALPHA * v.z;
            } else {
                const I_ALPHA: f64 = 0.10;
                let inv_dt = 1.0 / dt.max(1e-3);
                tr.vx = (1.0 - I_ALPHA) * tr.vx + I_ALPHA * innov_x * inv_dt;
                tr.vy = (1.0 - I_ALPHA) * tr.vy + I_ALPHA * innov_y * inv_dt;
            }
            tr.p = (tr.p * 0.6).max(4.0);
            tr.coast_ticks = 0;
            tr.hit_count += 1;
            tr.sensors.insert(d.sensor_kind);
            if d.rf_dark {
                tr.rf_dark_hits += 1;
            }
            if d.sensor_kind == SensorKind::Rf {
                tr.rf_hits += 1;
            }
            if let Some(c) = d.class_hypothesis {
                let conf = d.class_confidence.unwrap_or(0.3);
                if conf >= tr.class_confidence {
                    tr.class_hypothesis = c;
                    tr.class_confidence = conf;
                }
            }
            tr.affiliation = merge_affiliation(tr.affiliation, d.affiliation);
            tr.t = t;
        }

        for di in unused {
            let d = &detections[di];
            if d.class_hypothesis == Some(TrackClass::Bird) && d.sensor_kind == SensorKind::Radar {
                continue;
            }
            let v = d.velocity.unwrap_or(Vec3::zero());
            self.tracks.push(InternalTrack {
                id: self.ids.next(),
                t,
                x: d.position.x,
                y: d.position.y,
                z: d.position.z,
                vx: v.x,
                vy: v.y,
                vz: v.z,
                p: 40.0,
                class_hypothesis: d.class_hypothesis.unwrap_or(TrackClass::Unknown),
                class_confidence: d.class_confidence.unwrap_or(0.2),
                affiliation: d.affiliation,
                sensors: HashSet::from([d.sensor_kind]),
                hit_count: 1,
                coast_ticks: 0,
                born_t: t,
                rf_dark_hits: u32::from(d.rf_dark),
                rf_hits: u32::from(d.sensor_kind == SensorKind::Rf),
            });
        }

        self.refine_fiber_classes();

        self.tracks
            .retain(|tr| tr.coast_ticks <= self.coast_limit && tr.hit_count >= 1);

        self.export(t)
    }

    /// Radar + acoustic corroboration with RF absence boosts fiber-optic class.
    fn refine_fiber_classes(&mut self) {
        for tr in &mut self.tracks {
            let has_radar = tr.sensors.contains(&SensorKind::Radar);
            let has_acoustic = tr.sensors.contains(&SensorKind::Acoustic);
            let has_rf = tr.sensors.contains(&SensorKind::Rf) || tr.rf_hits > 0;
            let has_eo = tr.sensors.contains(&SensorKind::EoIr);

            if has_rf && tr.class_hypothesis == TrackClass::FiberOpticUas && tr.rf_hits >= 2 {
                // Sustained RF evidence demotes fiber hypothesis.
                tr.class_hypothesis = TrackClass::SwarmMember;
                tr.class_confidence = tr.class_confidence.min(0.55);
                continue;
            }

            if has_radar && has_acoustic && !has_rf && tr.hit_count >= 3 {
                let conf = 0.70 + if has_eo { 0.08 } else { 0.0 };
                let conf = (conf + (tr.rf_dark_hits as f64 * 0.02).min(0.08)).min(0.9);
                if matches!(
                    tr.class_hypothesis,
                    TrackClass::SwarmMember
                        | TrackClass::Multirotor
                        | TrackClass::Unknown
                        | TrackClass::FiberOpticUas
                ) && conf >= tr.class_confidence - 0.05
                {
                    tr.class_hypothesis = TrackClass::FiberOpticUas;
                    tr.class_confidence = conf.max(tr.class_confidence);
                }
            }
        }
    }

    fn export(&self, t: f64) -> Vec<Track> {
        self.tracks
            .iter()
            .filter(|tr| tr.hit_count >= 2 || tr.affiliation == Affiliation::Friendly)
            .map(|tr| {
                let rf_dark = tr.class_hypothesis == TrackClass::FiberOpticUas
                    || (tr.rf_dark_hits >= 2
                        && !tr.sensors.contains(&SensorKind::Rf)
                        && tr.hit_count >= 3);
                let track_status = if tr.coast_ticks > 0 {
                    TrackStatus::Coasting
                } else if tr.hit_count >= 4 {
                    TrackStatus::Confirmed
                } else {
                    TrackStatus::Tentative
                };
                let identity_basis = if tr.sensors.contains(&SensorKind::Adsb) {
                    IdentityBasis::Adsb
                } else if tr.sensors.contains(&SensorKind::EoIr) {
                    IdentityBasis::EoConfirmed
                } else if tr.sensors.contains(&SensorKind::Rf) {
                    IdentityBasis::RfSignature
                } else {
                    IdentityBasis::Unknown
                };
                let last_update_t = if tr.coast_ticks == 0 {
                    t
                } else {
                    (t - tr.coast_ticks as f64 * 0.05).max(tr.born_t)
                };
                Track {
                    id: tr.id,
                    t,
                    position: Vec3::new(tr.x, tr.y, tr.z),
                    velocity: Vec3::new(tr.vx, tr.vy, tr.vz),
                    covariance_trace: tr.p,
                    class_hypothesis: tr.class_hypothesis,
                    class_confidence: tr.class_confidence,
                    affiliation: tr.affiliation,
                    threat_score: 0.0,
                    sensor_provenance: {
                        let mut v: Vec<_> = tr.sensors.iter().copied().collect();
                        v.sort_by_key(|k| format!("{k:?}"));
                        v
                    },
                    hit_count: tr.hit_count,
                    coast_ticks: tr.coast_ticks,
                    age_s: (t - tr.born_t).max(0.0),
                    rf_dark,
                    track_status,
                    last_update_t,
                    nearest_asset_id: None,
                    zone_state: Default::default(),
                    identity_basis,
                    eta_s: None,
                }
            })
            .collect()
    }
}

fn sensor_gate_scale(kind: SensorKind) -> f64 {
    match kind {
        SensorKind::Adsb => 1.5,
        SensorKind::Acoustic => 2.6,
        SensorKind::EoIr => 0.85,
        SensorKind::Rf => 1.15,
        SensorKind::Radar => 1.0,
    }
}

fn associable(tr: &InternalTrack, d: &Detection, gate_m: f64) -> bool {
    let dist = hypot3(
        tr.x - d.position.x,
        tr.y - d.position.y,
        tr.z - d.position.z,
    );
    let gate = gate_m * sensor_gate_scale(d.sensor_kind);
    match d.sensor_kind {
        SensorKind::Adsb => dist <= gate,
        SensorKind::Acoustic => {
            if dist > gate {
                return false;
            }
            if let Some(brg) = d.bearing_rad {
                let range = d.range_m.unwrap_or(dist.max(1.0));
                let sx = d.position.x - range * brg.cos();
                let sy = d.position.y - range * brg.sin();
                let track_brg = (tr.y - sy).atan2(tr.x - sx);
                let mut d_ang = (track_brg - brg).abs();
                if d_ang > std::f64::consts::PI {
                    d_ang = std::f64::consts::TAU - d_ang;
                }
                if d_ang > 0.45 {
                    return false;
                }
            }
            velocity_consistent(tr, d)
        }
        _ => dist <= gate && velocity_consistent(tr, d),
    }
}

/// Reject associations whose reported velocity disagrees sharply with the track.
fn velocity_consistent(tr: &InternalTrack, d: &Detection) -> bool {
    let Some(v) = d.velocity else {
        return true;
    };
    let tr_speed = (tr.vx * tr.vx + tr.vy * tr.vy).sqrt();
    let d_speed = (v.x * v.x + v.y * v.y).sqrt();
    // Young / coasting tracks: skip velocity gate.
    if tr.hit_count < 3 || tr.coast_ticks > 8 {
        return true;
    }
    if tr_speed < 5.0 && d_speed < 5.0 {
        return true;
    }
    let dv = hypot3(tr.vx - v.x, tr.vy - v.y, tr.vz - v.z);
    // Allow large innov under maneuver; block only clear mismatches.
    dv <= 55.0 + 0.35 * tr_speed.max(d_speed)
}

fn association_cost(tr: &InternalTrack, d: &Detection) -> f64 {
    let dist = hypot3(
        tr.x - d.position.x,
        tr.y - d.position.y,
        tr.z - d.position.z,
    );
    let mut cost = dist;
    if let Some(v) = d.velocity {
        if tr.hit_count >= 3 {
            let dv = hypot3(tr.vx - v.x, tr.vy - v.y, tr.vz - v.z);
            cost += dv * 0.45;
        }
    }
    if d.sensor_kind == SensorKind::Acoustic {
        if let Some(brg) = d.bearing_rad {
            let range = d.range_m.unwrap_or(dist.max(1.0));
            let sx = d.position.x - range * brg.cos();
            let sy = d.position.y - range * brg.sin();
            let track_brg = (tr.y - sy).atan2(tr.x - sx);
            let mut d_ang = (track_brg - brg).abs();
            if d_ang > std::f64::consts::PI {
                d_ang = std::f64::consts::TAU - d_ang;
            }
            return dist * 0.35 + d_ang * 400.0 + (cost - dist);
        }
    }
    // Slight preference for multi-sensor corroboration continuity.
    if tr.sensors.contains(&d.sensor_kind) {
        cost *= 0.92;
    }
    cost
}

fn hypot3(x: f64, y: f64, z: f64) -> f64 {
    (x * x + y * y + z * z).sqrt()
}

fn merge_affiliation(a: Affiliation, b: Affiliation) -> Affiliation {
    use Affiliation::*;
    match (a, b) {
        (Friendly, _) | (_, Friendly) => Friendly,
        (Hostile, _) | (_, Hostile) => Hostile,
        (Neutral, other) | (other, Neutral) if other != Unknown => other,
        _ => Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aegis_schema::SensorKind;

    fn det(
        id: u128,
        kind: SensorKind,
        t: f64,
        x: f64,
        rf_dark: bool,
        class: TrackClass,
        conf: f64,
    ) -> Detection {
        Detection {
            id: Uuid::from_u128(id),
            sensor_id: format!("{kind:?}"),
            sensor_kind: kind,
            t,
            position: Vec3::new(x, 50.0, 120.0),
            velocity: Some(Vec3::new(-200.0, 0.0, 0.0)),
            range_m: Some(1000.0),
            bearing_rad: Some(50.0_f64.atan2(x)),
            elevation_rad: None,
            snr_db: Some(20.0),
            class_hypothesis: Some(class),
            class_confidence: Some(conf),
            affiliation: Affiliation::Unknown,
            rf_dark,
        }
    }

    #[test]
    fn forms_track_from_consistent_detections() {
        let mut eng = FusionEngine::new(1);
        let mut t = 0.0;
        let dt = 0.1;
        for i in 0..20 {
            t += dt;
            let x = 1000.0 - i as f64 * 20.0;
            let d = det(
                i as u128 + 1,
                SensorKind::Radar,
                t,
                x,
                false,
                TrackClass::SwarmMember,
                0.4,
            );
            let tracks = eng.process(t, dt, &[d]);
            if i > 3 {
                assert!(!tracks.is_empty());
            }
        }
    }

    #[test]
    fn track_ids_deterministic() {
        let mut a = FusionEngine::new(7);
        let mut b = FusionEngine::new(7);
        let d = det(
            99,
            SensorKind::Radar,
            0.1,
            100.0,
            false,
            TrackClass::SwarmMember,
            0.4,
        );
        let _ = a.process(0.1, 0.1, &[d.clone()]);
        let _ = b.process(0.1, 0.1, &[d.clone()]);
        let d2 = Detection {
            id: Uuid::from_u128(100),
            t: 0.2,
            position: Vec3::new(99.0, 0.0, 50.0),
            ..d
        };
        let ta = a.process(0.2, 0.1, &[d2.clone()]);
        let tb = b.process(0.2, 0.1, &[d2]);
        assert_eq!(ta[0].id, tb[0].id);
    }

    #[test]
    fn radar_acoustic_no_rf_yields_fiber_class() {
        let mut eng = FusionEngine::new(3);
        let mut t = 0.0;
        let dt = 0.2;
        let mut last = Vec::new();
        for i in 0..12 {
            t += dt;
            let x = 900.0 - i as f64 * 15.0;
            let radar = det(
                1000 + i as u128,
                SensorKind::Radar,
                t,
                x,
                true,
                TrackClass::FiberOpticUas,
                0.3,
            );
            let acoustic = Detection {
                id: Uuid::from_u128(2000 + i as u128),
                sensor_id: "acoustic".into(),
                sensor_kind: SensorKind::Acoustic,
                t,
                position: Vec3::new(x + 40.0, 60.0, 110.0),
                velocity: Some(Vec3::new(-80.0, 0.0, 0.0)),
                range_m: Some(950.0),
                bearing_rad: Some(60.0_f64.atan2(x + 40.0)),
                elevation_rad: None,
                snr_db: Some(16.0),
                class_hypothesis: Some(TrackClass::FiberOpticUas),
                class_confidence: Some(0.48),
                affiliation: Affiliation::Unknown,
                rf_dark: true,
            };
            last = eng.process(t, dt, &[radar, acoustic]);
        }
        assert!(!last.is_empty());
        let fiberish = last
            .iter()
            .any(|tr| tr.class_hypothesis == TrackClass::FiberOpticUas || tr.rf_dark);
        assert!(
            fiberish,
            "expected fiber class from radar+acoustic RF-dark cues"
        );
    }
}
