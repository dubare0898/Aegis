use crate::swarm::SwarmMember;
use cuas_schema::{DefeatCause, DefeatEvent, EffectorConfig, EffectorKind, EffectorStatus, Vec3};
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use std::collections::HashSet;
use uuid::Uuid;

const MAX_DEFEAT_EVENTS: usize = 40;

#[derive(Debug, Clone)]
struct InflightShot {
    target_truth_id: Uuid,
    impact_t: f64,
    pk: f64,
}

#[derive(Debug)]
pub struct EffectorRuntime {
    pub config: EffectorConfig,
    active_until: f64,
    cooldown_until: f64,
    tasked_track_id: Option<Uuid>,
    tasked_truth_id: Option<Uuid>,
    last_result: Option<String>,
    inflight: Option<InflightShot>,
}

impl EffectorRuntime {
    pub fn new(config: EffectorConfig) -> Self {
        Self {
            config,
            active_until: 0.0,
            cooldown_until: 0.0,
            tasked_track_id: None,
            tasked_truth_id: None,
            last_result: None,
            inflight: None,
        }
    }

    pub fn status(&self, t: f64) -> EffectorStatus {
        EffectorStatus {
            id: self.config.id.clone(),
            kind: self.config.kind,
            position: self.config.position,
            range_m: self.config.range_m,
            active: t < self.active_until || self.inflight.is_some(),
            tasked_track_id: self.tasked_track_id,
            cooldown_remaining_s: (self.cooldown_until - t).max(0.0),
            last_result: self.last_result.clone(),
        }
    }

    pub fn ready(&self, t: f64) -> bool {
        t >= self.cooldown_until && self.inflight.is_none()
    }
}

#[derive(Debug)]
pub struct EffectorSuite {
    pub units: Vec<EffectorRuntime>,
    defeat_events: Vec<DefeatEvent>,
    /// Idempotent key: (cause, truth_id)
    defeat_seen: HashSet<(DefeatCause, Uuid)>,
}

impl EffectorSuite {
    pub fn from_configs(configs: &[EffectorConfig]) -> Self {
        Self {
            units: configs.iter().cloned().map(EffectorRuntime::new).collect(),
            defeat_events: Vec::new(),
            defeat_seen: HashSet::new(),
        }
    }

    pub fn status(&self, t: f64) -> Vec<EffectorStatus> {
        self.units.iter().map(|u| u.status(t)).collect()
    }

    pub fn defeat_events(&self) -> &[DefeatEvent] {
        &self.defeat_events
    }

    fn push_defeat(
        &mut self,
        t: f64,
        truth_id: Uuid,
        label: impl Into<String>,
        cause: DefeatCause,
        rf_dark: bool,
        note: impl Into<String>,
    ) {
        let key = (cause, truth_id);
        if !self.defeat_seen.insert(key) {
            return;
        }
        self.defeat_events.push(DefeatEvent {
            t,
            truth_id,
            label: label.into(),
            cause,
            rf_dark,
            note: note.into(),
        });
        if self.defeat_events.len() > MAX_DEFEAT_EVENTS {
            let drain = self.defeat_events.len() - MAX_DEFEAT_EVENTS;
            for ev in self.defeat_events.drain(0..drain) {
                self.defeat_seen.remove(&(ev.cause, ev.truth_id));
            }
        }
    }

    /// Activate a jammer on the nearest in-range hostile to the track position.
    pub fn activate_jammer(
        &mut self,
        t: f64,
        track_id: Uuid,
        track_pos: Vec3,
        hostiles: &mut [SwarmMember],
    ) -> String {
        let Some(idx) = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| {
                u.config.kind == EffectorKind::Jammer
                    && u.ready(t)
                    && u.config.position.distance(&track_pos) <= u.config.range_m
            })
            .min_by(|(_, a), (_, b)| {
                a.config
                    .position
                    .distance(&track_pos)
                    .partial_cmp(&b.config.position.distance(&track_pos))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
        else {
            return "No jammer ready in range (cooldown or out of coverage)".into();
        };

        let range = self.units[idx].config.range_m;

        let Some(h_idx) = nearest_hostile(hostiles, track_pos, range + 200.0) else {
            return "No hostile truth near track for jammer task".into();
        };

        let h = &mut hostiles[h_idx];
        let dwell = self.units[idx].config.dwell_s;
        let cooldown = self.units[idx].config.cooldown_s;
        let jammer_id = self.units[idx].config.id.clone();
        let power = self.units[idx].config.power;
        self.units[idx].active_until = t + dwell;
        self.units[idx].cooldown_until = t + dwell + cooldown;
        self.units[idx].tasked_track_id = Some(track_id);
        self.units[idx].tasked_truth_id = Some(h.id);

        let (note, jam_ok) = if h.rf_dark {
            h.jammed = false;
            (
                format!("Jammer {jammer_id} tasked — RF-dark/fiber: soft-kill ineffective"),
                None,
            )
        } else {
            let label = h.label.clone();
            let tid = h.id;
            h.jammed = true;
            h.c2_degrade = 0.55 * power.clamp(0.4, 1.5);
            let note = format!("Jammer {jammer_id} active on {label} — C2/RF degraded");
            (note.clone(), Some((tid, label, note)))
        };
        self.units[idx].last_result = Some(note.clone());
        if let Some((tid, label, jam_note)) = jam_ok {
            self.push_defeat(t, tid, label, DefeatCause::Jamming, false, jam_note);
        }
        note
    }

    /// Fire kinetic effector at nearest hostile to track.
    pub fn fire_kinetic(
        &mut self,
        t: f64,
        track_id: Uuid,
        track_pos: Vec3,
        hostiles: &[SwarmMember],
        rng: &mut ChaCha8Rng,
    ) -> String {
        let Some(idx) = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| {
                u.config.kind == EffectorKind::Kinetic
                    && u.ready(t)
                    && u.config.position.distance(&track_pos) <= u.config.range_m
            })
            .min_by(|(_, a), (_, b)| {
                a.config
                    .position
                    .distance(&track_pos)
                    .partial_cmp(&b.config.position.distance(&track_pos))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
        else {
            return "No kinetic effector ready in range".into();
        };

        let range = self.units[idx].config.range_m;
        let pos = self.units[idx].config.position;

        let Some(h_idx) = nearest_hostile(hostiles, track_pos, range) else {
            return "No hostile truth near track for engage".into();
        };
        let h = &hostiles[h_idx];
        if h.neutralized {
            return format!("{} already neutralized", h.label);
        }

        let dist = pos.distance(&h.position);
        let mut pk =
            self.units[idx].config.pk_base * (1.0 - (dist / range) * 0.35).clamp(0.35, 1.0);
        if h.rf_dark {
            pk *= 0.9; // slightly harder visually/kinematically ambiguous fiber FPV
        }
        if h.role == "decoy" {
            pk *= 0.85;
        }
        // Deterministic-ish roll from rng
        let _ = rng.gen::<f64>(); // warm
        let roll_seed = rng.gen::<f64>();
        let tof = self.units[idx].config.tof_s;
        self.units[idx].inflight = Some(InflightShot {
            target_truth_id: h.id,
            impact_t: t + tof,
            pk,
        });
        self.units[idx].tasked_track_id = Some(track_id);
        let _ = track_id;
        self.units[idx].tasked_truth_id = Some(h.id);
        self.units[idx].active_until = t + tof;
        let note = format!(
            "{} launched at {} — TOF {:.1}s Pk≈{:.0}% (roll pending)",
            self.units[idx].config.id,
            h.label,
            tof,
            pk * 100.0
        );
        self.units[idx].last_result = Some(format!("In flight — Pk≈{:.0}%", pk * 100.0));
        let _ = roll_seed; // Pk applied at impact
        note
    }

    /// Tick jammer dwell / kinetic impacts.
    pub fn update(&mut self, t: f64, hostiles: &mut [SwarmMember], rng: &mut ChaCha8Rng) {
        // Collect kinetic hits then push defeats (avoid borrow conflicts).
        let mut kinetic_hits: Vec<(Uuid, String, bool)> = Vec::new();

        for u in &mut self.units {
            match u.config.kind {
                EffectorKind::Jammer => {
                    if t >= u.active_until {
                        if let Some(tid) = u.tasked_truth_id.take() {
                            if let Some(h) = hostiles.iter_mut().find(|h| h.id == tid) {
                                if !h.rf_dark {
                                    h.jammed = false;
                                    h.c2_degrade = 0.0;
                                }
                            }
                        }
                        u.tasked_track_id = None;
                    } else if let Some(tid) = u.tasked_truth_id {
                        if let Some(h) = hostiles.iter_mut().find(|h| h.id == tid) {
                            if !h.rf_dark && !h.neutralized {
                                h.jammed = true;
                                h.c2_degrade = 0.55 * u.config.power.clamp(0.4, 1.5);
                            }
                        }
                    }
                }
                EffectorKind::Kinetic => {
                    if let Some(shot) = u.inflight.clone() {
                        if t >= shot.impact_t {
                            u.inflight = None;
                            u.cooldown_until = t + u.config.cooldown_s;
                            u.tasked_track_id = None;
                            let hit = rng.gen::<f64>() < shot.pk;
                            if let Some(h) =
                                hostiles.iter_mut().find(|h| h.id == shot.target_truth_id)
                            {
                                if hit {
                                    h.neutralized = true;
                                    h.jammed = false;
                                    h.c2_degrade = 0.0;
                                    h.velocity = Vec3::zero();
                                    let note = format!("HIT — {} neutralized", h.label);
                                    u.last_result = Some(note.clone());
                                    kinetic_hits.push((h.id, h.label.clone(), h.rf_dark));
                                } else {
                                    u.last_result = Some(format!("MISS — {} continues", h.label));
                                }
                            } else {
                                u.last_result = Some("Impact — target lost".into());
                            }
                        }
                    }
                }
            }
        }

        for (tid, label, rf_dark) in kinetic_hits {
            self.push_defeat(
                t,
                tid,
                label.clone(),
                DefeatCause::Kinetic,
                rf_dark,
                format!("HIT — {label} neutralized"),
            );
        }
    }
}

fn nearest_hostile(hostiles: &[SwarmMember], pos: Vec3, max_dist: f64) -> Option<usize> {
    hostiles
        .iter()
        .enumerate()
        .filter(|(_, h)| !h.neutralized && h.position.distance(&pos) <= max_dist)
        .min_by(|(_, a), (_, b)| {
            a.position
                .distance(&pos)
                .partial_cmp(&b.position.distance(&pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
}
