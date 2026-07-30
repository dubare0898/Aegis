mod effectors;
mod sensors;
mod swarm;

use aegis_schema::{
    Affiliation, Detection, EffectorStatus, GoldenSnapshot, IdGen, ScenarioManifest, SensorKind,
    SensorStatus, Track, TrackClass, TruthEntity, Vec3, Zone,
};
use anyhow::{Context, Result};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use effectors::EffectorSuite;
use sensors::SensorRuntime;
use swarm::{SwarmMember, SwarmRuntime};

/// Stream id for entity / detection IDs (fusion uses a different stream).
pub const SIM_ID_STREAM: u64 = 1;

#[derive(Debug, Clone)]
pub struct SimConfig {
    pub scenario_dir: PathBuf,
    pub seed: u64,
}

#[derive(Debug)]
pub struct Simulation {
    pub manifest: ScenarioManifest,
    pub seed: u64,
    pub t: f64,
    pub tick: u64,
    pub dt: f64,
    pub running: bool,
    pub speed: f64,
    rng: ChaCha8Rng,
    ids: IdGen,
    swarm: SwarmRuntime,
    sensors: Vec<SensorRuntime>,
    friendlies: Vec<FriendlyEntity>,
    /// Static / slow fiber GCS / spool sites (weak radar, no RF, no air acoustic).
    ground: Vec<GroundEntity>,
    zones: Vec<Zone>,
    clutter_pool: Vec<ClutterPoint>,
    effectors: EffectorSuite,
    pub last_detections: Vec<Detection>,
    /// Soft Accept: AlertSector raises sensor attention (Pd) for a window.
    attention_boost_until: f64,
    /// Soft Accept: EvacuatePad moves friendlies clear of the pad.
    pad_evacuated: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct FriendlyEntity {
    pub id: Uuid,
    pub label: String,
    pub position: Vec3,
    pub velocity: Vec3,
    pub class: TrackClass,
}

#[derive(Debug, Clone)]
pub(crate) struct GroundEntity {
    pub id: Uuid,
    pub label: String,
    pub role: String,
    pub position: Vec3,
}

#[derive(Debug, Clone)]
pub(crate) struct ClutterPoint {
    pub position: Vec3,
}

#[derive(Debug, Deserialize)]
struct ScenarioFile {
    #[serde(flatten)]
    manifest: ScenarioManifest,
}

impl Simulation {
    pub fn load(scenario_dir: impl AsRef<Path>, seed: Option<u64>) -> Result<Self> {
        let scenario_dir = scenario_dir.as_ref().to_path_buf();
        let manifest_path = scenario_dir.join("scenario.json");
        let raw = fs::read_to_string(&manifest_path)
            .with_context(|| format!("reading {}", manifest_path.display()))?;
        let file: ScenarioFile = serde_json::from_str(&raw)
            .with_context(|| format!("parsing {}", manifest_path.display()))?;
        let manifest = file.manifest;
        let seed = seed.unwrap_or(manifest.default_seed);
        Ok(Self::from_manifest(manifest, seed))
    }

    pub fn from_manifest(manifest: ScenarioManifest, seed: u64) -> Self {
        let dt = 1.0 / manifest.tick_hz;
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut ids = IdGen::new(seed, SIM_ID_STREAM);
        let swarm = SwarmRuntime::spawn(&manifest.swarm, &mut rng, &mut ids);
        let mut sensors: Vec<SensorRuntime> = manifest
            .sensors
            .iter()
            .cloned()
            .map(SensorRuntime::new)
            .collect();
        let pd_s = manifest.environment.pd_scale.clamp(0.2, 1.5);
        let pfa_s = manifest.environment.pfa_scale.clamp(0.2, 4.0);
        for s in &mut sensors {
            s.config.pd = (s.config.pd * pd_s).clamp(0.05, 0.99);
            s.config.pfa = (s.config.pfa * pfa_s).clamp(0.0, 0.5);
        }
        let friendlies = manifest
            .friendlies
            .iter()
            .map(|f| FriendlyEntity {
                id: ids.next(),
                label: f.label.clone(),
                position: f.position,
                velocity: f.velocity,
                class: f.class,
            })
            .collect();
        let clutter_n = manifest.environment.clutter_count.max(1);
        let clutter_pool = (0..clutter_n)
            .map(|i| {
                let angle = (i as f64) * 0.7;
                let r = 800.0 + (i as f64) * 90.0;
                ClutterPoint {
                    position: Vec3::new(
                        r * angle.cos(),
                        r * angle.sin(),
                        40.0 + (i % 5) as f64 * 8.0,
                    ),
                }
            })
            .collect();

        let ground = spawn_fiber_spools(&manifest, &mut ids);
        let effectors = EffectorSuite::from_configs(&manifest.effectors);

        Self {
            zones: manifest.zones.clone(),
            manifest,
            seed,
            t: 0.0,
            tick: 0,
            dt,
            running: false,
            speed: 1.0,
            rng,
            ids,
            swarm,
            sensors,
            friendlies,
            ground,
            clutter_pool,
            effectors,
            last_detections: Vec::new(),
            attention_boost_until: 0.0,
            pad_evacuated: false,
        }
    }

    pub fn reset(&mut self, seed: Option<u64>) {
        let seed = seed.unwrap_or(self.seed);
        *self = Self::from_manifest(self.manifest.clone(), seed);
    }

    pub fn start(&mut self) {
        self.running = true;
    }

    pub fn pause(&mut self) {
        self.running = false;
    }

    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.clamp(0.25, 16.0);
    }

    pub fn cue_eo(&mut self, track_id: Uuid) {
        for sensor in &mut self.sensors {
            if sensor.config.kind == SensorKind::EoIr {
                sensor.tasked_track_id = Some(track_id);
            }
        }
    }

    pub fn fail_sensor(&mut self, sensor_id: &str) {
        if let Some(s) = self.sensors.iter_mut().find(|s| s.config.id == sensor_id) {
            s.healthy = false;
        }
    }

    pub fn restore_sensor(&mut self, sensor_id: &str) {
        if let Some(s) = self.sensors.iter_mut().find(|s| s.config.id == sensor_id) {
            s.healthy = true;
        }
    }

    pub fn set_eo_truth_targets(&mut self, tracks: &[(Uuid, Vec3)]) {
        for sensor in &mut self.sensors {
            if sensor.config.kind == SensorKind::EoIr {
                if let Some(id) = sensor.tasked_track_id {
                    if let Some((_, pos)) = tracks.iter().find(|(tid, _)| *tid == id) {
                        sensor.tasked_truth_pos = Some(*pos);
                    }
                }
            }
        }
    }

    pub fn track_position(&self, track_id: Uuid, tracks: &[(Uuid, Vec3)]) -> Option<Vec3> {
        tracks
            .iter()
            .find(|(id, _)| *id == track_id)
            .map(|(_, p)| *p)
            .or_else(|| {
                self.swarm
                    .members
                    .iter()
                    .find(|m| m.id == track_id)
                    .map(|m| m.position)
            })
    }

    /// Activate jammer against the hostile nearest the fused track position.
    pub fn activate_jammer(&mut self, track_id: Uuid, track_pos: Vec3) -> String {
        self.effectors
            .activate_jammer(self.t, track_id, track_pos, &mut self.swarm.members)
    }

    /// Fire kinetic effector at hostile nearest the fused track position.
    pub fn fire_kinetic(&mut self, track_id: Uuid, track_pos: Vec3) -> String {
        self.effectors.fire_kinetic(
            self.t,
            track_id,
            track_pos,
            &self.swarm.members,
            &mut self.rng,
        )
    }

    pub fn effector_status(&self) -> Vec<EffectorStatus> {
        self.effectors.status(self.t)
    }

    pub fn defeat_events(&self) -> &[aegis_schema::DefeatEvent] {
        self.effectors.defeat_events()
    }

    /// Soft Accept: raise radar/acoustic/RF Pd briefly (operator alerted the sector).
    pub fn apply_alert_sector(&mut self) {
        self.attention_boost_until = self.t + 45.0;
    }

    /// Soft Accept: evacuate pad friendlies outward from origin.
    pub fn apply_evacuate_pad(&mut self) {
        self.pad_evacuated = true;
        for f in &mut self.friendlies {
            let r = f.position.magnitude_xy().max(80.0);
            let scale = (r + 400.0) / r;
            f.position.x *= scale;
            f.position.y *= scale;
            // Push them further out over subsequent steps.
            let brg = f.position.y.atan2(f.position.x);
            f.velocity.x = 18.0 * brg.cos();
            f.velocity.y = 18.0 * brg.sin();
        }
    }

    pub fn pad_evacuated(&self) -> bool {
        self.pad_evacuated
    }

    pub fn attention_active(&self) -> bool {
        self.t < self.attention_boost_until
    }

    pub fn step(&mut self) -> Vec<Detection> {
        if !self.running {
            return self.last_detections.clone();
        }

        self.tick += 1;
        self.t += self.dt;

        self.effectors
            .update(self.t, &mut self.swarm.members, &mut self.rng);
        self.swarm.update(self.dt, self.t, &self.zones);
        for f in &mut self.friendlies {
            f.position.x += f.velocity.x * self.dt;
            f.position.y += f.velocity.y * self.dt;
            f.position.z += f.velocity.z * self.dt;
        }

        let mut detections = Vec::new();
        let hostiles: Vec<&SwarmMember> = self
            .swarm
            .members
            .iter()
            .filter(|m| !m.neutralized)
            .collect();

        let attention = self.t < self.attention_boost_until;
        for sensor in &mut self.sensors {
            let base_pd = sensor.config.pd;
            let base_pfa = sensor.config.pfa;
            if attention {
                // Alerted sector: operators lean on sensors — higher Pd, slightly higher Pfa.
                sensor.config.pd = (base_pd * 1.12).clamp(0.05, 0.99);
                sensor.config.pfa = (base_pfa * 1.15).clamp(0.0, 0.55);
            }
            let mut batch = sensor.sense(
                self.t,
                self.dt,
                &hostiles,
                &self.friendlies,
                &self.ground,
                &self.clutter_pool,
                &mut self.rng,
                &mut self.ids,
            );
            sensor.config.pd = base_pd;
            sensor.config.pfa = base_pfa;
            detections.append(&mut batch);
        }

        self.last_detections = detections.clone();
        detections
    }

    pub fn truth_entities(&self) -> Vec<TruthEntity> {
        let mut out = Vec::new();
        for m in &self.swarm.members {
            out.push(TruthEntity {
                id: m.id,
                label: m.label.clone(),
                role: m.role.clone(),
                position: m.position,
                velocity: m.velocity,
                affiliation: Affiliation::Hostile,
                class: if m.rf_dark {
                    TrackClass::FiberOpticUas
                } else if m.role == "decoy" {
                    TrackClass::Multirotor
                } else {
                    TrackClass::SwarmMember
                },
                rf_dark: m.rf_dark,
                jammed: m.jammed,
                neutralized: m.neutralized,
            });
        }
        for g in &self.ground {
            out.push(TruthEntity {
                id: g.id,
                label: g.label.clone(),
                role: g.role.clone(),
                position: g.position,
                velocity: Vec3::zero(),
                affiliation: Affiliation::Neutral,
                class: TrackClass::Unknown,
                rf_dark: true,
                jammed: false,
                neutralized: false,
            });
        }
        for f in &self.friendlies {
            out.push(TruthEntity {
                id: f.id,
                label: f.label.clone(),
                role: "friendly".into(),
                position: f.position,
                velocity: f.velocity,
                affiliation: Affiliation::Friendly,
                class: f.class,
                rf_dark: false,
                jammed: false,
                neutralized: false,
            });
        }
        out
    }

    pub fn sensor_status(&self) -> Vec<SensorStatus> {
        self.sensors
            .iter()
            .map(|s| SensorStatus {
                id: s.config.id.clone(),
                kind: s.config.kind,
                position: s.config.position,
                range_m: s.config.range_m,
                healthy: s.healthy,
                tasked_track_id: s.tasked_track_id,
            })
            .collect()
    }

    pub fn zones(&self) -> &[Zone] {
        &self.zones
    }

    pub fn hostile_positions(&self) -> Vec<(Uuid, Vec3)> {
        self.swarm
            .members
            .iter()
            .map(|m| (m.id, m.position))
            .collect()
    }

    /// Build a golden-comparable snapshot (optionally with fused tracks).
    pub fn golden_snapshot(&self, tracks: &[Track]) -> GoldenSnapshot {
        GoldenSnapshot {
            seed: self.seed,
            tick: self.tick,
            t: self.t,
            truth: self.truth_entities(),
            detections: self.last_detections.clone(),
            tracks: tracks.to_vec(),
        }
    }
}

fn spawn_fiber_spools(manifest: &ScenarioManifest, ids: &mut IdGen) -> Vec<GroundEntity> {
    if manifest.swarm.fiber_fraction <= 0.0 {
        return Vec::new();
    }
    let bearing = manifest.swarm.ingress_bearing_deg.to_radians();
    let r = (manifest.swarm.start_range_m * 0.92).max(800.0);
    let lateral = 220.0;
    vec![
        GroundEntity {
            id: ids.next(),
            label: "FIBER-GCS-1".into(),
            role: "fiber_gcs".into(),
            position: Vec3::new(
                r * bearing.cos() - lateral * bearing.sin(),
                r * bearing.sin() + lateral * bearing.cos(),
                2.0,
            ),
        },
        GroundEntity {
            id: ids.next(),
            label: "FIBER-SPOOL-2".into(),
            role: "fiber_gcs".into(),
            position: Vec3::new(
                r * bearing.cos() + lateral * 0.6 * bearing.sin(),
                r * bearing.sin() - lateral * 0.6 * bearing.cos(),
                2.0,
            ),
        },
    ]
}

/// Resolve scenario directory relative to workspace root or CWD.
pub fn resolve_scenario_dir(name: &str) -> PathBuf {
    let candidates = [
        PathBuf::from("scenarios").join(name),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../scenarios")
            .join(name),
    ];
    for c in candidates {
        if c.join("scenario.json").exists() {
            return c;
        }
    }
    PathBuf::from("scenarios").join(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aegis_fusion::FusionEngine;

    fn run_pipeline(seed: u64, ticks: u64) -> (GoldenSnapshot, GoldenSnapshot) {
        let dir = resolve_scenario_dir("military-base-swarm");
        let mut a = Simulation::load(&dir, Some(seed)).expect("load a");
        let mut b = Simulation::load(&dir, Some(seed)).expect("load b");
        let mut fa = FusionEngine::new(seed);
        let mut fb = FusionEngine::new(seed);
        a.start();
        b.start();
        let mut tracks_a = Vec::new();
        let mut tracks_b = Vec::new();
        for _ in 0..ticks {
            let da = a.step();
            let db = b.step();
            tracks_a = fa.process(a.t, a.dt, &da);
            tracks_b = fb.process(b.t, b.dt, &db);
        }
        (a.golden_snapshot(&tracks_a), b.golden_snapshot(&tracks_b))
    }

    #[test]
    fn deterministic_replay_positions() {
        let dir = resolve_scenario_dir("military-base-swarm");
        let mut a = Simulation::load(&dir, Some(42)).expect("load a");
        let mut b = Simulation::load(&dir, Some(42)).expect("load b");
        a.start();
        b.start();
        for _ in 0..120 {
            let da = a.step();
            let db = b.step();
            assert_eq!(da.len(), db.len());
            for (x, y) in da.iter().zip(db.iter()) {
                assert_eq!(x.id, y.id);
                assert!((x.position.x - y.position.x).abs() < 1e-9);
                assert!((x.position.y - y.position.y).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn golden_snapshot_bit_identical() {
        let (sa, sb) = run_pipeline(42, 120);
        let ja = serde_json::to_string(&sa).unwrap();
        let jb = serde_json::to_string(&sb).unwrap();
        assert_eq!(ja, jb);
        assert_eq!(sa, sb);
        assert!(!sa.truth.is_empty());
        assert!(!sa.detections.is_empty() || sa.tick > 0);
    }

    #[test]
    fn jammer_degrades_rf_hostile_not_fiber() {
        let dir = resolve_scenario_dir("military-base-swarm");
        let mut sim = Simulation::load(&dir, Some(42)).expect("load");
        sim.start();
        for _ in 0..80 {
            let _ = sim.step();
        }
        let rf = sim
            .swarm
            .members
            .iter()
            .find(|m| !m.rf_dark && !m.neutralized)
            .map(|m| (m.id, m.position))
            .expect("rf hostile");
        let fiber = sim
            .swarm
            .members
            .iter()
            .find(|m| m.rf_dark && !m.neutralized)
            .map(|m| (m.id, m.position))
            .expect("fiber hostile");

        let note_rf = sim.activate_jammer(Uuid::nil(), rf.1);
        assert!(
            note_rf.to_lowercase().contains("degraded") || note_rf.contains("C2"),
            "rf jam note: {note_rf}"
        );
        assert!(
            sim.swarm.members.iter().any(|m| m.id == rf.0 && m.jammed),
            "rf hostile should be jammed"
        );

        // Second jammer for fiber (first may be on cooldown after dwell assignment)
        let note_fiber = sim.activate_jammer(Uuid::nil(), fiber.1);
        assert!(
            note_fiber.to_lowercase().contains("ineffective")
                || note_fiber.to_lowercase().contains("rf-dark"),
            "fiber jam note: {note_fiber}"
        );
        assert!(
            sim.swarm
                .members
                .iter()
                .any(|m| m.id == fiber.0 && !m.jammed),
            "fiber should not be jammed"
        );
    }

    #[test]
    fn kinetic_engage_can_neutralize() {
        let dir = resolve_scenario_dir("military-base-swarm");
        let mut sim = Simulation::load(&dir, Some(7)).expect("load");
        sim.start();
        for _ in 0..100 {
            let _ = sim.step();
        }
        let target = sim
            .swarm
            .members
            .iter()
            .find(|m| !m.neutralized)
            .map(|m| (m.id, m.position))
            .expect("hostile");
        let note = sim.fire_kinetic(Uuid::nil(), target.1);
        assert!(note.contains("launched") || note.contains("TOF"), "{note}");
        // Advance past TOF
        for _ in 0..80 {
            let _ = sim.step();
        }
        let hit_or_miss = sim
            .effector_status()
            .iter()
            .filter(|e| e.kind == aegis_schema::EffectorKind::Kinetic)
            .any(|e| {
                e.last_result
                    .as_deref()
                    .map(|r| r.contains("HIT") || r.contains("MISS"))
                    .unwrap_or(false)
            });
        assert!(hit_or_miss, "expected kinetic hit/miss result");
        if sim.swarm.members.iter().any(|m| m.neutralized) {
            assert!(
                sim.defeat_events()
                    .iter()
                    .any(|e| e.cause == aegis_schema::DefeatCause::Kinetic),
                "neutralized hostile should emit kinetic defeat"
            );
        }
    }

    #[test]
    fn jammer_emits_defeat_for_rf_not_fiber() {
        let dir = resolve_scenario_dir("military-base-swarm");
        let mut sim = Simulation::load(&dir, Some(42)).expect("load");
        sim.start();
        for _ in 0..80 {
            let _ = sim.step();
        }
        let rf = sim
            .swarm
            .members
            .iter()
            .find(|m| !m.rf_dark && !m.neutralized)
            .map(|m| m.position)
            .expect("rf hostile");
        let _ = sim.activate_jammer(Uuid::nil(), rf);
        assert!(
            sim.defeat_events()
                .iter()
                .any(|e| e.cause == aegis_schema::DefeatCause::Jamming && !e.rf_dark),
            "RF jam should emit jamming defeat"
        );
        let before = sim.defeat_events().len();
        let fiber = sim
            .swarm
            .members
            .iter()
            .find(|m| m.rf_dark && !m.neutralized)
            .map(|m| m.position)
            .expect("fiber");
        // Wait for jammer cooldown
        for _ in 0..400 {
            let _ = sim.step();
        }
        let _ = sim.activate_jammer(Uuid::nil(), fiber);
        let fiber_jams = sim
            .defeat_events()
            .iter()
            .filter(|e| e.cause == aegis_schema::DefeatCause::Jamming && e.rf_dark)
            .count();
        assert_eq!(fiber_jams, 0, "fiber must not count as jamming defeat");
        assert!(
            sim.defeat_events().len() >= before,
            "fiber jam must not clear prior defeats"
        );
    }
}
