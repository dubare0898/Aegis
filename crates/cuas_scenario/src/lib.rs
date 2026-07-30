//! Seeded scenario generation: ScenarioClass + constraints + seed → ScenarioManifest.
//!
//! Determinism: the same `(class, seed)` always yields bit-identical JSON.
//! Runtime sim RNG should use the same seed for full replay.

use cuas_schema::{
    EnvironmentConfig, FaultEvent, FaultPolicy, FriendlyConfig, ScenarioClass, ScenarioConstraints,
    ScenarioManifest, SpawnCorridor, SwarmConfig, TrackClass, Vec3,
};
use cuas_sim::resolve_scenario_dir;
use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScenarioGenError {
    #[error("load base template: {0}")]
    Load(String),
    #[error("invalid generated scenario: {0}")]
    Invalid(String),
}

fn stream_rng(seed: u64, stream: u64) -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(seed ^ stream.wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

pub fn constraints_for(class: ScenarioClass) -> ScenarioConstraints {
    let base_corridor = SpawnCorridor {
        bearing_deg: 215.0,
        bearing_jitter_deg: 12.0,
        range_min_m: 2400.0,
        range_max_m: 3600.0,
        spread_min_m: 100.0,
        spread_max_m: 280.0,
    };
    match class {
        ScenarioClass::DirectSwarmRaid => ScenarioConstraints {
            count_min: 10,
            count_max: 16,
            decoy_fraction_min: 0.05,
            decoy_fraction_max: 0.2,
            fiber_fraction_min: 0.0,
            fiber_fraction_max: 0.15,
            speed_min_mps: 30.0,
            speed_max_mps: 40.0,
            altitude_min_m: 120.0,
            altitude_max_m: 200.0,
            corridor: base_corridor,
            behavior: Default::default(),
        },
        ScenarioClass::MixedRfDarkRaid => ScenarioConstraints {
            count_min: 10,
            count_max: 16,
            decoy_fraction_min: 0.1,
            decoy_fraction_max: 0.3,
            fiber_fraction_min: 0.25,
            fiber_fraction_max: 0.55,
            speed_min_mps: 28.0,
            speed_max_mps: 38.0,
            altitude_min_m: 100.0,
            altitude_max_m: 190.0,
            corridor: SpawnCorridor {
                range_min_m: 2200.0,
                range_max_m: 3200.0,
                ..base_corridor
            },
            behavior: Default::default(),
        },
        ScenarioClass::DecoyScreen => ScenarioConstraints {
            count_min: 12,
            count_max: 20,
            decoy_fraction_min: 0.45,
            decoy_fraction_max: 0.7,
            fiber_fraction_min: 0.05,
            fiber_fraction_max: 0.25,
            speed_min_mps: 26.0,
            speed_max_mps: 36.0,
            altitude_min_m: 110.0,
            altitude_max_m: 220.0,
            corridor: base_corridor,
            behavior: Default::default(),
        },
        ScenarioClass::ClutterHeavyFalseAlarmDay => ScenarioConstraints {
            count_min: 8,
            count_max: 14,
            decoy_fraction_min: 0.15,
            decoy_fraction_max: 0.35,
            fiber_fraction_min: 0.1,
            fiber_fraction_max: 0.3,
            speed_min_mps: 28.0,
            speed_max_mps: 36.0,
            altitude_min_m: 120.0,
            altitude_max_m: 180.0,
            corridor: base_corridor,
            behavior: Default::default(),
        },
        ScenarioClass::FriendlyCrossingWithHostileIngress => ScenarioConstraints {
            count_min: 10,
            count_max: 14,
            decoy_fraction_min: 0.15,
            decoy_fraction_max: 0.35,
            fiber_fraction_min: 0.15,
            fiber_fraction_max: 0.4,
            speed_min_mps: 30.0,
            speed_max_mps: 38.0,
            altitude_min_m: 130.0,
            altitude_max_m: 190.0,
            corridor: base_corridor,
            behavior: Default::default(),
        },
        ScenarioClass::DegradedSensorDefense => ScenarioConstraints {
            count_min: 10,
            count_max: 14,
            decoy_fraction_min: 0.15,
            decoy_fraction_max: 0.3,
            fiber_fraction_min: 0.2,
            fiber_fraction_max: 0.4,
            speed_min_mps: 30.0,
            speed_max_mps: 38.0,
            altitude_min_m: 120.0,
            altitude_max_m: 180.0,
            corridor: base_corridor,
            behavior: Default::default(),
        },
    }
}

pub fn load_base_template() -> Result<ScenarioManifest, ScenarioGenError> {
    let dir = resolve_scenario_dir("military-base-swarm");
    let path = dir.join("scenario.json");
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| ScenarioGenError::Load(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&raw).map_err(|e| ScenarioGenError::Load(e.to_string()))
}

/// Generate a resolved ScenarioManifest (instance) from class + seed.
pub fn generate(class: ScenarioClass, seed: u64) -> Result<ScenarioManifest, ScenarioGenError> {
    generate_with_constraints(class, &constraints_for(class), seed)
}

pub fn generate_with_constraints(
    class: ScenarioClass,
    constraints: &ScenarioConstraints,
    seed: u64,
) -> Result<ScenarioManifest, ScenarioGenError> {
    validate_constraints(constraints)?;

    let mut manifest = load_base_template()?;
    let mut pkg_rng = stream_rng(seed, 1);
    let mut friendly_rng = stream_rng(seed, 2);
    let mut clutter_rng = stream_rng(seed, 3);
    let mut fault_rng = stream_rng(seed, 4);
    let mut env_rng = stream_rng(seed, 5);

    // --- Hostile package ---
    let count = pkg_rng.gen_range(constraints.count_min..=constraints.count_max);
    let decoy = pkg_rng.gen_range(constraints.decoy_fraction_min..=constraints.decoy_fraction_max);
    let fiber = pkg_rng.gen_range(constraints.fiber_fraction_min..=constraints.fiber_fraction_max);
    // Remaining fraction after decoy must fit fiber.
    let fiber = fiber.min((1.0 - decoy).max(0.0));
    let bearing = constraints.corridor.bearing_deg
        + pkg_rng.gen_range(
            -constraints.corridor.bearing_jitter_deg..=constraints.corridor.bearing_jitter_deg,
        );
    let range =
        pkg_rng.gen_range(constraints.corridor.range_min_m..=constraints.corridor.range_max_m);
    let spread =
        pkg_rng.gen_range(constraints.corridor.spread_min_m..=constraints.corridor.spread_max_m);
    let speed = pkg_rng.gen_range(constraints.speed_min_mps..=constraints.speed_max_mps);
    let alt = pkg_rng.gen_range(constraints.altitude_min_m..=constraints.altitude_max_m);

    // Aggression slightly scales cruise speed within bounds.
    let speed = (speed * (0.92 + 0.16 * constraints.behavior.aggression))
        .clamp(constraints.speed_min_mps, constraints.speed_max_mps);

    manifest.swarm = SwarmConfig {
        count,
        ingress_bearing_deg: bearing,
        start_range_m: range,
        cruise_speed_mps: speed,
        altitude_m: alt,
        spread_m: spread,
        decoy_fraction: decoy,
        fiber_fraction: fiber,
    };

    // --- Friendlies ---
    match class {
        ScenarioClass::FriendlyCrossingWithHostileIngress => {
            let side = if friendly_rng.gen_bool(0.5) {
                1.0
            } else {
                -1.0
            };
            let px = friendly_rng.gen_range(-2200.0..-800.0);
            let py = side * friendly_rng.gen_range(1600.0..2800.0);
            let vx = friendly_rng.gen_range(40.0..70.0);
            let vy = -side * friendly_rng.gen_range(10.0..35.0);
            manifest.friendlies = vec![
                FriendlyConfig {
                    label: "MED-EVAC-1".into(),
                    position: Vec3::new(px, py, friendly_rng.gen_range(350.0..480.0)),
                    velocity: Vec3::new(vx, vy, 0.0),
                    class: TrackClass::Manned,
                },
                FriendlyConfig {
                    label: "RESUPPLY-2".into(),
                    position: Vec3::new(
                        px + 400.0,
                        -py * 0.6,
                        friendly_rng.gen_range(300.0..420.0),
                    ),
                    velocity: Vec3::new(vx * 0.85, -vy * 0.5, 0.0),
                    class: TrackClass::Manned,
                },
            ];
        }
        _ => {
            // Keep single friendly; jitter path slightly.
            if let Some(f) = manifest.friendlies.first_mut() {
                f.position.x += friendly_rng.gen_range(-120.0..120.0);
                f.position.y += friendly_rng.gen_range(-120.0..120.0);
                f.velocity.x += friendly_rng.gen_range(-5.0..5.0);
                f.velocity.y += friendly_rng.gen_range(-5.0..5.0);
            }
        }
    }

    // --- Environment / clutter ---
    let mut env = EnvironmentConfig::default();
    match class {
        ScenarioClass::ClutterHeavyFalseAlarmDay => {
            env.clutter_count = clutter_rng.gen_range(48..=72);
            env.pfa_scale = env_rng.gen_range(2.0..=3.2);
            env.pd_scale = env_rng.gen_range(0.9..=1.05);
        }
        ScenarioClass::MixedRfDarkRaid => {
            env.clutter_count = clutter_rng.gen_range(20..=32);
            // Slight acoustic bias already in base; nudge Pd up lightly.
            env.pd_scale = env_rng.gen_range(1.0..=1.08);
        }
        _ => {
            env.clutter_count = clutter_rng.gen_range(18..=28);
            let _ = env_rng.gen::<f64>();
        }
    }
    manifest.environment = env;

    // --- Faults ---
    manifest.fault_policy = match class {
        ScenarioClass::DegradedSensorDefense => {
            let fail_at = fault_rng.gen_range(1.5..=4.0);
            let restore_at = fail_at + fault_rng.gen_range(6.0..=14.0);
            FaultPolicy {
                events: vec![FaultEvent {
                    sensor_id: "radar-north".into(),
                    fail_at_s: fail_at,
                    restore_at_s: Some(restore_at),
                }],
            }
        }
        _ => {
            // Consume stream for determinism even when unused.
            let _ = fault_rng.gen::<f64>();
            FaultPolicy::default()
        }
    };

    manifest.scenario_class = Some(class);
    manifest.id = format!("{}-{}", class.as_str(), seed);
    manifest.name = format!("{} (seed {seed})", class.as_str());
    manifest.description = format!(
        "Generated {} from military-base-swarm template; seed={seed}",
        class.as_str()
    );
    manifest.default_seed = seed;

    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_constraints(c: &ScenarioConstraints) -> Result<(), ScenarioGenError> {
    if c.count_min == 0 || c.count_min > c.count_max {
        return Err(ScenarioGenError::Invalid("count bounds".into()));
    }
    if c.corridor.range_min_m <= 0.0 || c.corridor.range_min_m > c.corridor.range_max_m {
        return Err(ScenarioGenError::Invalid("corridor range".into()));
    }
    if c.speed_min_mps <= 0.0 || c.speed_min_mps > c.speed_max_mps {
        return Err(ScenarioGenError::Invalid("speed bounds".into()));
    }
    if c.decoy_fraction_min < 0.0 || c.decoy_fraction_max > 1.0 {
        return Err(ScenarioGenError::Invalid("decoy fraction".into()));
    }
    Ok(())
}

pub fn validate_manifest(m: &ScenarioManifest) -> Result<(), ScenarioGenError> {
    if m.zones.is_empty() {
        return Err(ScenarioGenError::Invalid("no zones".into()));
    }
    if m.sensors.is_empty() {
        return Err(ScenarioGenError::Invalid("no sensors".into()));
    }
    let extent = m.site.extent_m;
    for s in &m.sensors {
        if s.position.magnitude_xy() > extent * 1.5 {
            return Err(ScenarioGenError::Invalid(format!(
                "sensor {} outside site",
                s.id
            )));
        }
        if s.range_m <= 0.0 || !s.pd.is_finite() {
            return Err(ScenarioGenError::Invalid(format!(
                "sensor {} kinematics/pd",
                s.id
            )));
        }
    }
    let sw = &m.swarm;
    if sw.count == 0 || sw.start_range_m < 200.0 || sw.cruise_speed_mps < 5.0 {
        return Err(ScenarioGenError::Invalid("swarm spawn/kinematics".into()));
    }
    if sw.decoy_fraction < 0.0
        || sw.fiber_fraction < 0.0
        || sw.decoy_fraction + sw.fiber_fraction > 1.0 + 1e-6
    {
        return Err(ScenarioGenError::Invalid("swarm fractions".into()));
    }
    if !(20.0..=500.0).contains(&sw.altitude_m) {
        return Err(ScenarioGenError::Invalid("altitude".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_identical_json() {
        let a = generate(ScenarioClass::MixedRfDarkRaid, 42).unwrap();
        let b = generate(ScenarioClass::MixedRfDarkRaid, 42).unwrap();
        let ja = serde_json::to_string(&a).unwrap();
        let jb = serde_json::to_string(&b).unwrap();
        assert_eq!(ja, jb);
    }

    #[test]
    fn different_seeds_differ() {
        let a = generate(ScenarioClass::DirectSwarmRaid, 1).unwrap();
        let b = generate(ScenarioClass::DirectSwarmRaid, 2).unwrap();
        assert_ne!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }

    #[test]
    fn mixed_rf_dark_has_fiber() {
        let m = generate(ScenarioClass::MixedRfDarkRaid, 19).unwrap();
        assert!(m.swarm.fiber_fraction > 0.0);
        assert_eq!(m.scenario_class, Some(ScenarioClass::MixedRfDarkRaid));
    }

    #[test]
    fn degraded_has_fault_policy() {
        let m = generate(ScenarioClass::DegradedSensorDefense, 7).unwrap();
        assert!(!m.fault_policy.events.is_empty());
        assert_eq!(m.fault_policy.events[0].sensor_id, "radar-north");
    }

    #[test]
    fn invalid_constraints_rejected() {
        let mut c = constraints_for(ScenarioClass::DirectSwarmRaid);
        c.count_min = 10;
        c.count_max = 2;
        assert!(generate_with_constraints(ScenarioClass::DirectSwarmRaid, &c, 1).is_err());
    }
}
