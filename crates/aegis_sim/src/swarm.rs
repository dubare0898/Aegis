use aegis_schema::{IdGen, SwarmConfig, Vec3, Zone, ZoneKind};
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use uuid::Uuid;

// Steering tunables (module constants — no scenario schema change).
const SEPARATION_RADIUS_M: f64 = 95.0;
const ALIGNMENT_RADIUS_M: f64 = 220.0;
const COHESION_RADIUS_M: f64 = 320.0;
const MAX_ACCEL_MPS2: f64 = 14.0;
const MAX_TURN_RATE_RAD_S: f64 = 0.85;
const WANDER_UPDATE_S: f64 = 2.4;
const WANDER_STRENGTH: f64 = 4.5;
const TERMINAL_ATTACK_RADIUS_M: f64 = 900.0;
const HYSTERESIS_MARGIN_M: f64 = 80.0;
const LATERAL_DAMP: f64 = 0.92;
const AVOID_ENTER_FRAC: f64 = 0.55;
const AVOID_EXIT_FRAC: f64 = 0.72;

#[derive(Debug, Clone)]
pub struct SwarmMember {
    pub id: Uuid,
    pub label: String,
    pub role: String,
    pub position: Vec3,
    pub velocity: Vec3,
    pub target: Vec3,
    pub split_phase: f64,
    /// Fiber-tethered / RF-dark platform.
    pub rf_dark: bool,
    pub jammed: bool,
    /// 0 = nominal, higher = stronger C2 degradation while jammed.
    pub c2_degrade: f64,
    pub neutralized: bool,
    /// Cruise / max speed for this member (set at spawn).
    pub max_speed_mps: f64,
    /// Low-frequency wander heading bias (radians).
    pub wander_theta: f64,
    /// Next sim time to refresh wander (deterministic timer).
    pub wander_next_t: f64,
    /// Hysteresis latch for soft keep-out avoidance.
    pub avoid_latch: bool,
}

#[derive(Debug)]
pub struct SwarmRuntime {
    pub members: Vec<SwarmMember>,
}

#[derive(Clone, Copy)]
struct Snapshot {
    id: Uuid,
    position: Vec3,
    velocity: Vec3,
    neutralized: bool,
}

impl SwarmRuntime {
    pub fn spawn(cfg: &SwarmConfig, rng: &mut ChaCha8Rng, ids: &mut IdGen) -> Self {
        let bearing = cfg.ingress_bearing_deg.to_radians();
        let mut members = Vec::with_capacity(cfg.count);
        let decoy_count = ((cfg.count as f64) * cfg.decoy_fraction)
            .round()
            .clamp(0.0, cfg.count as f64) as usize;
        let remaining = cfg.count.saturating_sub(decoy_count);
        let fiber_count = ((cfg.count as f64) * cfg.fiber_fraction)
            .round()
            .clamp(0.0, remaining as f64) as usize;

        for i in 0..cfg.count {
            let side = if i % 2 == 0 { 1.0 } else { -1.0 };
            let lateral = side * cfg.spread_m * (0.3 + (i as f64) * 0.08);
            let along = cfg.start_range_m + (i as f64) * 35.0;
            let x = along * bearing.cos() - lateral * bearing.sin();
            let y = along * bearing.sin() + lateral * bearing.cos();
            let z = cfg.altitude_m + rng.gen_range(-25.0..25.0);

            let role = if i < decoy_count {
                "decoy".to_string()
            } else if i < decoy_count + fiber_count {
                "fiber_optic".to_string()
            } else {
                "strike".to_string()
            };
            let rf_dark = role == "fiber_optic";

            let target = if role == "decoy" {
                Vec3::new(
                    rng.gen_range(-400.0..400.0),
                    rng.gen_range(-400.0..400.0),
                    cfg.altitude_m + 80.0,
                )
            } else {
                Vec3::new(
                    rng.gen_range(-120.0..120.0),
                    rng.gen_range(-120.0..120.0),
                    40.0 + rng.gen_range(0.0..40.0),
                )
            };

            let dx = target.x - x;
            let dy = target.y - y;
            let dist = (dx * dx + dy * dy).sqrt().max(1.0);
            let speed = cfg.cruise_speed_mps
                * if role == "decoy" {
                    0.85
                } else if rf_dark {
                    0.95
                } else {
                    1.05
                };

            members.push(SwarmMember {
                id: ids.next(),
                label: format!("H-{:02}", i + 1),
                role,
                position: Vec3::new(x, y, z),
                velocity: Vec3::new(speed * dx / dist, speed * dy / dist, 0.0),
                target,
                split_phase: rng.gen_range(0.0..std::f64::consts::TAU),
                rf_dark,
                jammed: false,
                c2_degrade: 0.0,
                neutralized: false,
                max_speed_mps: speed,
                wander_theta: rng.gen_range(0.0..std::f64::consts::TAU),
                wander_next_t: rng.gen_range(0.4..WANDER_UPDATE_S),
                avoid_latch: false,
            });
        }

        Self { members }
    }

    pub fn update(&mut self, dt: f64, t: f64, zones: &[Zone]) {
        let asset = zones
            .iter()
            .find(|z| z.kind == ZoneKind::CriticalAsset)
            .map(|z| z.center)
            .unwrap_or(Vec3::zero());
        let keep_out = zones
            .iter()
            .find(|z| z.kind == ZoneKind::KeepOut || z.kind == ZoneKind::NoFly);

        let snaps: Vec<Snapshot> = self
            .members
            .iter()
            .map(|m| Snapshot {
                id: m.id,
                position: m.position,
                velocity: m.velocity,
                neutralized: m.neutralized,
            })
            .collect();

        for m in self.members.iter_mut() {
            if m.neutralized {
                m.velocity = Vec3::zero();
                continue;
            }

            // Deterministic wander refresh (no RNG in update).
            if t >= m.wander_next_t {
                let step = 0.55
                    + 0.35 * (m.split_phase + m.wander_theta + t * 0.17).sin()
                    + if m.role == "decoy" { 0.45 } else { 0.0 };
                m.wander_theta = wrap_pi(m.wander_theta + step);
                m.wander_next_t =
                    t + WANDER_UPDATE_S * (0.85 + 0.3 * (m.split_phase * 0.7).cos().abs());
            }
            m.split_phase += dt * 0.12;

            let weights = role_weights(m);
            let aim = if m.role == "decoy" { m.target } else { asset };
            let to_aim = Vec3::new(
                aim.x - m.position.x,
                aim.y - m.position.y,
                aim.z - m.position.z,
            );
            let dist_xy = to_aim.magnitude_xy().max(1.0);
            let terminal = dist_xy < TERMINAL_ATTACK_RADIUS_M;
            let seek_w = if terminal {
                weights.seek * 1.55
            } else {
                weights.seek
            };

            let mut desired = scale_xy(&unit_xy(&to_aim), m.max_speed_mps * seek_w);

            // Separation / alignment / cohesion from neighbors.
            let mut sep = Vec3::zero();
            let mut sep_n = 0.0;
            let mut align = Vec3::zero();
            let mut align_n = 0.0;
            let mut coh = Vec3::zero();
            let mut coh_n = 0.0;

            for n in &snaps {
                if n.id == m.id || n.neutralized {
                    continue;
                }
                let d = m.position.distance_xy(&n.position);
                if d < SEPARATION_RADIUS_M && d > 1e-3 {
                    let push = (SEPARATION_RADIUS_M - d) / SEPARATION_RADIUS_M;
                    sep.x += (m.position.x - n.position.x) / d * push;
                    sep.y += (m.position.y - n.position.y) / d * push;
                    sep_n += 1.0;
                }
                if d < ALIGNMENT_RADIUS_M {
                    align.x += n.velocity.x;
                    align.y += n.velocity.y;
                    align_n += 1.0;
                }
                if d < COHESION_RADIUS_M {
                    coh.x += n.position.x;
                    coh.y += n.position.y;
                    coh_n += 1.0;
                }
            }

            if sep_n > 0.0 {
                desired = add_xy(
                    &desired,
                    &scale_xy(&unit_xy(&sep), m.max_speed_mps * weights.separation),
                );
            }
            if align_n > 0.0 {
                let avg = Vec3::new(align.x / align_n, align.y / align_n, 0.0);
                desired = add_xy(
                    &desired,
                    &scale_xy(&unit_xy(&avg), m.max_speed_mps * weights.alignment),
                );
            }
            if coh_n > 0.0 {
                let center = Vec3::new(coh.x / coh_n, coh.y / coh_n, 0.0);
                let to_c = Vec3::new(center.x - m.position.x, center.y - m.position.y, 0.0);
                desired = add_xy(
                    &desired,
                    &scale_xy(&unit_xy(&to_c), m.max_speed_mps * weights.cohesion),
                );
            }

            // Soft keep-out avoidance with hysteresis (not a hard bounce).
            if let Some(z) = keep_out {
                let r = m.position.distance_xy(&z.center);
                let enter_r = z.radius_m * AVOID_ENTER_FRAC;
                let exit_r = z.radius_m * AVOID_EXIT_FRAC + HYSTERESIS_MARGIN_M;
                if r < enter_r {
                    m.avoid_latch = true;
                } else if r > exit_r {
                    m.avoid_latch = false;
                }
                // Fiber/strike still ingress: only a light tangential push early on.
                if m.avoid_latch && !terminal && !m.rf_dark {
                    let away = Vec3::new(m.position.x - z.center.x, m.position.y - z.center.y, 0.0);
                    desired = add_xy(&desired, &scale_xy(&unit_xy(&away), m.max_speed_mps * 0.22));
                }
            }

            // Slow wander bias — decoys noisier; fiber quieter / more direct.
            let wander_s = WANDER_STRENGTH
                * weights.wander
                * if m.jammed {
                    // Mild, low-frequency effect — no high-rate weave snaps.
                    1.0 + m.c2_degrade * 0.35
                } else {
                    1.0
                };
            desired.x += wander_s * m.wander_theta.cos();
            desired.y += wander_s * m.wander_theta.sin();

            // Jam: reduce max speed, not heading jitter.
            let speed_scale = if m.jammed {
                (1.0 - m.c2_degrade * 0.65).clamp(0.25, 1.0)
            } else {
                1.0
            };
            let max_speed = m.max_speed_mps * speed_scale;
            desired = clamp_speed_xy(&desired, max_speed);

            // Vertical: gentle climb/descend toward aim altitude.
            let desired_vz = ((aim.z - m.position.z) * 0.35).clamp(-6.0, 6.0);

            // Accel toward desired with turn-rate limit on heading.
            let mut new_v = steer_toward(&m.velocity, &desired, desired_vz, dt, max_speed);

            // Damp lateral oscillation relative to seek direction.
            let seek_u = unit_xy(&to_aim);
            let along = new_v.x * seek_u.x + new_v.y * seek_u.y;
            let lat_x = new_v.x - along * seek_u.x;
            let lat_y = new_v.y - along * seek_u.y;
            new_v.x = along * seek_u.x + lat_x * LATERAL_DAMP;
            new_v.y = along * seek_u.y + lat_y * LATERAL_DAMP;
            new_v = clamp_speed_xy(&new_v, max_speed);
            new_v.z = desired_vz;

            m.velocity = new_v;
            m.position.x += m.velocity.x * dt;
            m.position.y += m.velocity.y * dt;
            m.position.z += m.velocity.z * dt;
            m.position.z = m.position.z.clamp(25.0, 450.0);
        }
    }
}

struct RoleWeights {
    seek: f64,
    separation: f64,
    alignment: f64,
    cohesion: f64,
    wander: f64,
}

fn role_weights(m: &SwarmMember) -> RoleWeights {
    if m.role == "decoy" {
        RoleWeights {
            seek: 0.55,
            separation: 0.9,
            alignment: 0.35,
            cohesion: 0.25,
            wander: 1.6,
        }
    } else if m.rf_dark || m.role == "fiber_optic" {
        RoleWeights {
            seek: 1.15,
            separation: 0.85,
            alignment: 0.55,
            cohesion: 0.4,
            wander: 0.45,
        }
    } else {
        // strike / swarm member — coordinated ingress
        RoleWeights {
            seek: 0.95,
            separation: 1.0,
            alignment: 0.85,
            cohesion: 0.7,
            wander: 0.75,
        }
    }
}

fn unit_xy(v: &Vec3) -> Vec3 {
    let m = v.magnitude_xy();
    if m < 1e-6 {
        Vec3::zero()
    } else {
        Vec3::new(v.x / m, v.y / m, 0.0)
    }
}

fn scale_xy(v: &Vec3, s: f64) -> Vec3 {
    Vec3::new(v.x * s, v.y * s, 0.0)
}

fn add_xy(a: &Vec3, b: &Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, 0.0)
}

fn clamp_speed_xy(v: &Vec3, max_speed: f64) -> Vec3 {
    let speed = v.magnitude_xy();
    if speed <= max_speed || speed < 1e-9 {
        *v
    } else {
        let s = max_speed / speed;
        Vec3::new(v.x * s, v.y * s, v.z)
    }
}

fn heading(v: &Vec3) -> f64 {
    v.y.atan2(v.x)
}

fn wrap_pi(a: f64) -> f64 {
    let mut x = a;
    while x > std::f64::consts::PI {
        x -= std::f64::consts::TAU;
    }
    while x < -std::f64::consts::PI {
        x += std::f64::consts::TAU;
    }
    x
}

fn steer_toward(current: &Vec3, desired: &Vec3, desired_vz: f64, dt: f64, max_speed: f64) -> Vec3 {
    let cur_speed = current.magnitude_xy().max(1e-6);
    let des_speed = desired.magnitude_xy().min(max_speed);
    let cur_h = heading(current);
    let des_h = if desired.magnitude_xy() < 1e-6 {
        cur_h
    } else {
        heading(desired)
    };

    let mut dh = wrap_pi(des_h - cur_h);
    let max_turn = MAX_TURN_RATE_RAD_S * dt;
    dh = dh.clamp(-max_turn, max_turn);
    let new_h = cur_h + dh;

    let speed_err = des_speed - cur_speed;
    let max_dv = MAX_ACCEL_MPS2 * dt;
    let new_speed = (cur_speed + speed_err.clamp(-max_dv, max_dv)).clamp(0.0, max_speed);

    let vz_err = desired_vz - current.z;
    let new_vz = current.z + vz_err.clamp(-max_dv, max_dv);

    Vec3::new(new_speed * new_h.cos(), new_speed * new_h.sin(), new_vz)
}
