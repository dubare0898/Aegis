use crate::swarm::SwarmMember;
use crate::{ClutterPoint, FriendlyEntity, GroundEntity};
use cuas_schema::{Affiliation, Detection, IdGen, SensorConfig, SensorKind, TrackClass, Vec3};
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use uuid::Uuid;

#[derive(Debug)]
pub struct SensorRuntime {
    pub config: SensorConfig,
    pub healthy: bool,
    pub tasked_track_id: Option<Uuid>,
    pub tasked_truth_pos: Option<Vec3>,
    accum: f64,
}

impl SensorRuntime {
    pub fn new(config: SensorConfig) -> Self {
        Self {
            config,
            healthy: true,
            tasked_track_id: None,
            tasked_truth_pos: None,
            accum: 0.0,
        }
    }

    pub fn sense(
        &mut self,
        t: f64,
        dt: f64,
        hostiles: &[&SwarmMember],
        friendlies: &[FriendlyEntity],
        ground: &[GroundEntity],
        clutter: &[ClutterPoint],
        rng: &mut ChaCha8Rng,
        ids: &mut IdGen,
    ) -> Vec<Detection> {
        if !self.healthy {
            return Vec::new();
        }

        self.accum += dt;
        let period = 1.0 / self.config.update_hz.max(0.1);
        if self.accum < period {
            return Vec::new();
        }
        self.accum -= period;

        match self.config.kind {
            SensorKind::Radar => self.sense_radar(t, hostiles, ground, clutter, rng, ids),
            SensorKind::Rf => self.sense_rf(t, hostiles, rng, ids),
            SensorKind::EoIr => self.sense_eo(t, hostiles, rng, ids),
            SensorKind::Adsb => self.sense_adsb(t, friendlies, ids),
            SensorKind::Acoustic => self.sense_acoustic(t, hostiles, rng, ids),
        }
    }

    fn sense_radar(
        &self,
        t: f64,
        hostiles: &[&SwarmMember],
        ground: &[GroundEntity],
        clutter: &[ClutterPoint],
        rng: &mut ChaCha8Rng,
        ids: &mut IdGen,
    ) -> Vec<Detection> {
        let mut out = Vec::new();
        for h in hostiles {
            if h.neutralized {
                continue;
            }
            let dist = self.config.position.distance(&h.position);
            if dist > self.config.range_m {
                continue;
            }
            let range_factor = 1.0 - (dist / self.config.range_m) * 0.35;
            let pd = (self.config.pd * range_factor).clamp(0.05, 0.99);
            if rng.gen::<f64>() > pd {
                continue;
            }
            let noisy = jitter(h.position, self.config.position_sigma_m, rng);
            let (class, conf) = if h.rf_dark {
                (TrackClass::FiberOpticUas, 0.28)
            } else {
                (TrackClass::SwarmMember, 0.35)
            };
            out.push(base_detection(
                &self.config,
                t,
                noisy,
                Some(h.velocity),
                Affiliation::Unknown,
                Some(class),
                Some(conf),
                h.rf_dark,
                ids,
            ));
        }

        // Weak ground returns on fiber GCS / spool sites (no air acoustic signature).
        for g in ground {
            let dist = self.config.position.distance(&g.position);
            if dist > self.config.range_m * 0.45 {
                continue;
            }
            if rng.gen::<f64>() > 0.22 {
                continue;
            }
            let noisy = jitter(g.position, self.config.position_sigma_m * 1.8, rng);
            out.push(base_detection(
                &self.config,
                t,
                noisy,
                None,
                Affiliation::Neutral,
                Some(TrackClass::Unknown),
                Some(0.2),
                true,
                ids,
            ));
        }

        if !clutter.is_empty() && rng.gen::<f64>() < self.config.pfa {
            let c = &clutter[rng.gen_range(0..clutter.len())];
            let noisy = jitter(c.position, self.config.position_sigma_m * 1.5, rng);
            out.push(base_detection(
                &self.config,
                t,
                noisy,
                None,
                Affiliation::Unknown,
                Some(TrackClass::Bird),
                Some(0.15),
                false,
                ids,
            ));
        }
        out
    }

    fn sense_rf(
        &self,
        t: f64,
        hostiles: &[&SwarmMember],
        rng: &mut ChaCha8Rng,
        ids: &mut IdGen,
    ) -> Vec<Detection> {
        let mut out = Vec::new();
        for h in hostiles {
            if h.neutralized {
                continue;
            }
            // Fiber-optic platforms are RF-dark; tiny residual for motor EMI / accidental emitters.
            // Jammed RF platforms drop emit probability hard (C2 lost).
            let emit_p = if h.rf_dark {
                0.02
            } else if h.jammed {
                0.04
            } else if h.role == "decoy" {
                0.25
            } else {
                0.55
            };
            let dist = self.config.position.distance(&h.position);
            if dist > self.config.range_m || rng.gen::<f64>() > emit_p {
                continue;
            }
            if rng.gen::<f64>() > self.config.pd {
                continue;
            }
            let noisy = jitter(h.position, self.config.position_sigma_m, rng);
            out.push(base_detection(
                &self.config,
                t,
                noisy,
                None,
                Affiliation::Hostile,
                Some(TrackClass::Multirotor),
                Some(0.55),
                false,
                ids,
            ));
        }
        out
    }

    fn sense_acoustic(
        &self,
        t: f64,
        hostiles: &[&SwarmMember],
        rng: &mut ChaCha8Rng,
        ids: &mut IdGen,
    ) -> Vec<Detection> {
        let mut out = Vec::new();
        // Ambient noise reduces Pd (MVP scalar; not a full noise model).
        let ambient = 0.18;
        for h in hostiles {
            if h.neutralized {
                continue;
            }
            let dist = self.config.position.distance(&h.position);
            if dist > self.config.range_m {
                continue;
            }
            let range_factor = (1.0 - (dist / self.config.range_m) * 0.75).clamp(0.08, 1.0);
            let role_boost = if h.rf_dark { 1.35 } else { 1.0 };
            let pd =
                (self.config.pd * range_factor * role_boost * (1.0 - ambient)).clamp(0.05, 0.95);
            if rng.gen::<f64>() > pd {
                continue;
            }

            let dx = h.position.x - self.config.position.x;
            let dy = h.position.y - self.config.position.y;
            let true_bearing = dy.atan2(dx);
            // Bearing-heavy: larger angle noise, weaker range.
            let bearing = true_bearing + rng.gen_range(-0.22..0.22);
            let range_noisy =
                (dist + rng.gen_range(-1.0..1.0) * self.config.position_sigma_m * 2.4).max(40.0);
            let z_noisy =
                (h.position.z + rng.gen_range(-1.0..1.0) * self.config.position_sigma_m).max(10.0);
            let noisy = Vec3::new(
                self.config.position.x + range_noisy * bearing.cos(),
                self.config.position.y + range_noisy * bearing.sin(),
                z_noisy,
            );

            let (class, conf) = if h.rf_dark {
                (TrackClass::FiberOpticUas, 0.48)
            } else {
                (TrackClass::Multirotor, 0.32)
            };
            out.push(base_detection(
                &self.config,
                t,
                noisy,
                Some(h.velocity),
                Affiliation::Unknown,
                Some(class),
                Some(conf),
                h.rf_dark,
                ids,
            ));
        }
        out
    }

    fn sense_eo(
        &self,
        t: f64,
        hostiles: &[&SwarmMember],
        rng: &mut ChaCha8Rng,
        ids: &mut IdGen,
    ) -> Vec<Detection> {
        let Some(target_pos) = self.tasked_truth_pos.or_else(|| {
            self.tasked_track_id
                .and_then(|_| hostiles.first().map(|h| h.position))
        }) else {
            return Vec::new();
        };

        let dist = self.config.position.distance(&target_pos);
        if dist > self.config.range_m {
            return Vec::new();
        }
        if rng.gen::<f64>() > self.config.pd {
            return Vec::new();
        }

        let nearest_rf_dark = hostiles
            .iter()
            .filter(|h| h.rf_dark)
            .min_by(|a, b| {
                a.position
                    .distance(&target_pos)
                    .partial_cmp(&b.position.distance(&target_pos))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .filter(|h| h.position.distance(&target_pos) < 120.0);

        let noisy = jitter(target_pos, self.config.position_sigma_m * 0.4, rng);
        let (class, conf, rf_dark) = if let Some(h) = nearest_rf_dark {
            (TrackClass::FiberOpticUas, 0.8, h.rf_dark)
        } else if rng.gen::<f64>() > 0.3 {
            (TrackClass::SwarmMember, 0.82, false)
        } else {
            (TrackClass::Multirotor, 0.82, false)
        };
        vec![base_detection(
            &self.config,
            t,
            noisy,
            None,
            Affiliation::Hostile,
            Some(class),
            Some(conf),
            rf_dark,
            ids,
        )]
    }

    fn sense_adsb(&self, t: f64, friendlies: &[FriendlyEntity], ids: &mut IdGen) -> Vec<Detection> {
        friendlies
            .iter()
            .filter(|f| self.config.position.distance(&f.position) <= self.config.range_m)
            .map(|f| {
                base_detection(
                    &self.config,
                    t,
                    f.position,
                    Some(f.velocity),
                    Affiliation::Friendly,
                    Some(f.class),
                    Some(0.95),
                    false,
                    ids,
                )
            })
            .collect()
    }
}

fn jitter(pos: Vec3, sigma: f64, rng: &mut ChaCha8Rng) -> Vec3 {
    let mut noise = || {
        let u: f64 = rng.gen::<f64>() + rng.gen::<f64>() + rng.gen::<f64>() - 1.5;
        u * sigma
    };
    Vec3::new(
        pos.x + noise(),
        pos.y + noise(),
        (pos.z + noise() * 0.4).max(5.0),
    )
}

fn base_detection(
    cfg: &SensorConfig,
    t: f64,
    position: Vec3,
    velocity: Option<Vec3>,
    affiliation: Affiliation,
    class_hypothesis: Option<TrackClass>,
    class_confidence: Option<f64>,
    rf_dark: bool,
    ids: &mut IdGen,
) -> Detection {
    let dx = position.x - cfg.position.x;
    let dy = position.y - cfg.position.y;
    let dz = position.z - cfg.position.z;
    let range = (dx * dx + dy * dy + dz * dz).sqrt();
    Detection {
        id: ids.next(),
        sensor_id: cfg.id.clone(),
        sensor_kind: cfg.kind,
        t,
        position,
        velocity,
        range_m: Some(range),
        bearing_rad: Some(dy.atan2(dx)),
        elevation_rad: Some(dz.atan2((dx * dx + dy * dy).sqrt())),
        snr_db: Some(18.0 + snr_boost(cfg.kind)),
        class_hypothesis,
        class_confidence,
        affiliation,
        rf_dark,
    }
}

fn snr_boost(kind: SensorKind) -> f64 {
    match kind {
        SensorKind::Radar => 6.0,
        SensorKind::Rf => 2.0,
        SensorKind::EoIr => 10.0,
        SensorKind::Adsb => 12.0,
        SensorKind::Acoustic => 4.0,
    }
}
