use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }

    pub fn distance_xy(&self, other: &Vec3) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }

    pub fn distance(&self, other: &Vec3) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    pub fn magnitude_xy(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SensorKind {
    Radar,
    Rf,
    EoIr,
    Adsb,
    Acoustic,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Affiliation {
    Hostile,
    Friendly,
    Unknown,
    Neutral,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrackClass {
    FixedWingUav,
    Multirotor,
    SwarmMember,
    FiberOpticUas,
    Manned,
    Bird,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Detection {
    pub id: Uuid,
    pub sensor_id: String,
    pub sensor_kind: SensorKind,
    pub t: f64,
    pub position: Vec3,
    pub velocity: Option<Vec3>,
    pub range_m: Option<f64>,
    pub bearing_rad: Option<f64>,
    pub elevation_rad: Option<f64>,
    pub snr_db: Option<f64>,
    pub class_hypothesis: Option<TrackClass>,
    pub class_confidence: Option<f64>,
    pub affiliation: Affiliation,
    /// True when the emitter/platform is believed RF-dark (e.g. fiber-tethered).
    #[serde(default)]
    pub rf_dark: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrackStatus {
    #[default]
    Tentative,
    Confirmed,
    Coasting,
    Dropped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ZoneState {
    #[default]
    Outside,
    Watch,
    Warning,
    Defended,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum IdentityBasis {
    Adsb,
    RfSignature,
    EoConfirmed,
    OperatorLabeled,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub id: Uuid,
    pub t: f64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub covariance_trace: f64,
    pub class_hypothesis: TrackClass,
    pub class_confidence: f64,
    pub affiliation: Affiliation,
    pub threat_score: f64,
    pub sensor_provenance: Vec<SensorKind>,
    pub hit_count: u32,
    pub coast_ticks: u32,
    pub age_s: f64,
    /// Inferred RF-dark / fiber-tethered hypothesis.
    #[serde(default)]
    pub rf_dark: bool,
    #[serde(default)]
    pub track_status: TrackStatus,
    #[serde(default)]
    pub last_update_t: f64,
    #[serde(default)]
    pub nearest_asset_id: Option<String>,
    #[serde(default)]
    pub zone_state: ZoneState,
    #[serde(default)]
    pub identity_basis: IdentityBasis,
    /// Estimated seconds to critical asset along closing trajectory (`None` if outbound / not closing).
    #[serde(default)]
    pub eta_s: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RecommendedAction {
    CueEo,
    AlertSector,
    RequestJammerAuthorization,
    EvacuatePad,
    HandOffHigherEchelon,
    EngageKinetic,
    MaintainWatch,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationStatus {
    #[default]
    Open,
    Accepted,
    Rejected,
    Deferred,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperatorDisposition {
    Accepted,
    Rejected,
    Deferred,
}

impl OperatorDisposition {
    pub fn to_status(self) -> RecommendationStatus {
        match self {
            Self::Accepted => RecommendationStatus::Accepted,
            Self::Rejected => RecommendationStatus::Rejected,
            Self::Deferred => RecommendationStatus::Deferred,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Criticality {
    #[default]
    Soft,
    MissionCritical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecommendedBy {
    #[default]
    Rules,
    Fusion,
    OperatorAssist,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceItem {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OperatorActor {
    #[default]
    Operator,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DispositionReasonCode {
    ApprovedBestOption,
    InsufficientConfidence,
    FalsePositiveSuspected,
    WaitingForHigher,
    TrainingOnly,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub id: Uuid,
    pub track_id: Uuid,
    pub t: f64,
    pub priority: u32,
    pub threat_score: f64,
    pub action: RecommendedAction,
    pub title: String,
    pub rationale: Vec<String>,
    pub confidence: f64,
    pub uncertainty: String,
    #[serde(default)]
    pub status: RecommendationStatus,
    #[serde(default)]
    pub disposed_at: Option<f64>,
    #[serde(default)]
    pub criticality: Criticality,
    #[serde(default)]
    pub expected_effect: String,
    #[serde(default)]
    pub recommended_by: RecommendedBy,
    #[serde(default)]
    pub evidence: Vec<EvidenceItem>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    #[serde(default)]
    pub requires_confirmation: bool,
    /// Estimated seconds to critical asset for the associated track (copied for cards).
    #[serde(default)]
    pub eta_s: Option<f64>,
    /// Short "why this rank" line (ETA / closing / zone / conservation).
    #[serde(default)]
    pub rank_rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorEvent {
    pub t: f64,
    pub track_id: Uuid,
    pub recommendation_id: Uuid,
    pub action: RecommendedAction,
    pub disposition: OperatorDisposition,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub actor: OperatorActor,
    #[serde(default)]
    pub reason_code: Option<DispositionReasonCode>,
}

/// Operator / doctrine state plus soft flags (effectors live in EffectorStatus).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OperatorState {
    pub alerted_zone_ids: Vec<String>,
    pub jammer_auth_track_ids: Vec<Uuid>,
    pub evacuated_pad: bool,
    pub handed_off_track_ids: Vec<Uuid>,
    pub neutralized_truth_ids: Vec<Uuid>,
}

/// How a hostile was defeated / suppressed for the downed panel.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DefeatCause {
    Kinetic,
    Jamming,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DefeatEvent {
    pub t: f64,
    pub truth_id: Uuid,
    pub label: String,
    pub cause: DefeatCause,
    #[serde(default)]
    pub rf_dark: bool,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EffectorKind {
    Jammer,
    Kinetic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectorConfig {
    pub id: String,
    pub kind: EffectorKind,
    pub position: Vec3,
    pub range_m: f64,
    /// Jammer dwell / weapon engagement window (s).
    #[serde(default = "default_dwell_s")]
    pub dwell_s: f64,
    #[serde(default = "default_cooldown_s")]
    pub cooldown_s: f64,
    /// Jammer relative power (1.0 = nominal).
    #[serde(default = "default_power")]
    pub power: f64,
    /// Kinetic base Pk at short range.
    #[serde(default = "default_pk")]
    pub pk_base: f64,
    /// Kinetic time-of-flight (s).
    #[serde(default = "default_tof_s")]
    pub tof_s: f64,
}

fn default_dwell_s() -> f64 {
    12.0
}
fn default_cooldown_s() -> f64 {
    8.0
}
fn default_power() -> f64 {
    1.0
}
fn default_pk() -> f64 {
    0.72
}
fn default_tof_s() -> f64 {
    2.5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectorStatus {
    pub id: String,
    pub kind: EffectorKind,
    pub position: Vec3,
    pub range_m: f64,
    pub active: bool,
    pub tasked_track_id: Option<Uuid>,
    pub cooldown_remaining_s: f64,
    #[serde(default)]
    pub last_result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorStatus {
    pub id: String,
    pub kind: SensorKind,
    pub position: Vec3,
    pub range_m: f64,
    pub healthy: bool,
    pub tasked_track_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Zone {
    pub id: String,
    pub name: String,
    pub kind: ZoneKind,
    pub center: Vec3,
    pub radius_m: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ZoneKind {
    KeepOut,
    NoFly,
    CriticalAsset,
    BaseFootprint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TruthEntity {
    pub id: Uuid,
    pub label: String,
    pub role: String,
    pub position: Vec3,
    pub velocity: Vec3,
    pub affiliation: Affiliation,
    pub class: TrackClass,
    #[serde(default)]
    pub rf_dark: bool,
    #[serde(default)]
    pub jammed: bool,
    #[serde(default)]
    pub neutralized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirPicture {
    pub t: f64,
    pub tick: u64,
    pub running: bool,
    pub speed: f64,
    pub seed: u64,
    pub scenario_id: String,
    pub detections: Vec<Detection>,
    pub tracks: Vec<Track>,
    pub recommendations: Vec<Recommendation>,
    pub sensors: Vec<SensorStatus>,
    pub zones: Vec<Zone>,
    pub truth: Vec<TruthEntity>,
    #[serde(default)]
    pub operator_events: Vec<OperatorEvent>,
    #[serde(default)]
    pub operator_state: OperatorState,
    #[serde(default)]
    pub effectors: Vec<EffectorStatus>,
    /// Soft/hard defeats attributed to effectors (jammer / kinetic).
    #[serde(default)]
    pub defeat_events: Vec<DefeatEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientCommand {
    Start,
    Pause,
    SetSpeed {
        speed: f64,
    },
    Reset {
        seed: Option<u64>,
    },
    CueEo {
        track_id: Uuid,
    },
    FailSensor {
        sensor_id: String,
    },
    RestoreSensor {
        sensor_id: String,
    },
    DisposeRecommendation {
        recommendation_id: Uuid,
        disposition: OperatorDisposition,
        #[serde(default)]
        actor: OperatorActor,
        #[serde(default)]
        reason_code: Option<DispositionReasonCode>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Snapshot(AirPicture),
    Info { message: String },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioManifest {
    pub id: String,
    pub name: String,
    pub vertical: String,
    pub description: String,
    pub tick_hz: f64,
    pub default_seed: u64,
    pub site: SiteConfig,
    pub sensors: Vec<SensorConfig>,
    pub zones: Vec<Zone>,
    pub swarm: SwarmConfig,
    pub friendlies: Vec<FriendlyConfig>,
    pub roe_profile: String,
    #[serde(default)]
    pub effectors: Vec<EffectorConfig>,
    /// Optional scheduled sensor faults (generated instances / degraded class).
    #[serde(default)]
    pub fault_policy: FaultPolicy,
    /// Clutter / Pd-Pfa environment modifiers.
    #[serde(default)]
    pub environment: EnvironmentConfig,
    /// Scenario class tag when produced by the generator.
    #[serde(default)]
    pub scenario_class: Option<ScenarioClass>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioClass {
    DirectSwarmRaid,
    MixedRfDarkRaid,
    DecoyScreen,
    ClutterHeavyFalseAlarmDay,
    FriendlyCrossingWithHostileIngress,
    DegradedSensorDefense,
}

impl ScenarioClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DirectSwarmRaid => "direct_swarm_raid",
            Self::MixedRfDarkRaid => "mixed_rf_dark_raid",
            Self::DecoyScreen => "decoy_screen",
            Self::ClutterHeavyFalseAlarmDay => "clutter_heavy_false_alarm_day",
            Self::FriendlyCrossingWithHostileIngress => "friendly_crossing_with_hostile_ingress",
            Self::DegradedSensorDefense => "degraded_sensor_defense",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "direct_swarm_raid" => Some(Self::DirectSwarmRaid),
            "mixed_rf_dark_raid" => Some(Self::MixedRfDarkRaid),
            "decoy_screen" => Some(Self::DecoyScreen),
            "clutter_heavy_false_alarm_day" => Some(Self::ClutterHeavyFalseAlarmDay),
            "friendly_crossing_with_hostile_ingress" => {
                Some(Self::FriendlyCrossingWithHostileIngress)
            }
            "degraded_sensor_defense" => Some(Self::DegradedSensorDefense),
            "all" => None,
            _ => None,
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::DirectSwarmRaid,
            Self::MixedRfDarkRaid,
            Self::DecoyScreen,
            Self::ClutterHeavyFalseAlarmDay,
            Self::FriendlyCrossingWithHostileIngress,
            Self::DegradedSensorDefense,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpawnCorridor {
    pub bearing_deg: f64,
    pub bearing_jitter_deg: f64,
    pub range_min_m: f64,
    pub range_max_m: f64,
    pub spread_min_m: f64,
    pub spread_max_m: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostilePackage {
    pub count: usize,
    pub decoy_fraction: f64,
    pub fiber_fraction: f64,
    pub corridor: SpawnCorridor,
    pub cruise_speed_mps: f64,
    pub altitude_m: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BehaviorProfile {
    pub aggression: f64,
    pub cohesion: f64,
    pub wander: f64,
}

impl Default for BehaviorProfile {
    fn default() -> Self {
        Self {
            aggression: 0.7,
            cohesion: 0.65,
            wander: 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FaultEvent {
    pub sensor_id: String,
    pub fail_at_s: f64,
    #[serde(default)]
    pub restore_at_s: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FaultPolicy {
    #[serde(default)]
    pub events: Vec<FaultEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvironmentConfig {
    #[serde(default = "default_clutter_count")]
    pub clutter_count: usize,
    #[serde(default = "default_one")]
    pub pfa_scale: f64,
    #[serde(default = "default_one")]
    pub pd_scale: f64,
}

fn default_clutter_count() -> usize {
    24
}
fn default_one() -> f64 {
    1.0
}

impl Default for EnvironmentConfig {
    fn default() -> Self {
        Self {
            clutter_count: default_clutter_count(),
            pfa_scale: 1.0,
            pd_scale: 1.0,
        }
    }
}

/// Bounds for seeded generation (class supplies defaults).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScenarioConstraints {
    pub count_min: usize,
    pub count_max: usize,
    pub decoy_fraction_min: f64,
    pub decoy_fraction_max: f64,
    pub fiber_fraction_min: f64,
    pub fiber_fraction_max: f64,
    pub speed_min_mps: f64,
    pub speed_max_mps: f64,
    pub altitude_min_m: f64,
    pub altitude_max_m: f64,
    pub corridor: SpawnCorridor,
    pub behavior: BehaviorProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteConfig {
    pub name: String,
    pub origin_lat: f64,
    pub origin_lon: f64,
    pub extent_m: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorConfig {
    pub id: String,
    pub kind: SensorKind,
    pub position: Vec3,
    pub range_m: f64,
    pub pd: f64,
    pub pfa: f64,
    pub position_sigma_m: f64,
    pub update_hz: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmConfig {
    pub count: usize,
    pub ingress_bearing_deg: f64,
    pub start_range_m: f64,
    pub cruise_speed_mps: f64,
    pub altitude_m: f64,
    pub spread_m: f64,
    pub decoy_fraction: f64,
    /// Fraction of swarm that are RF-dark fiber-optic UAS (0.0–1.0).
    #[serde(default)]
    pub fiber_fraction: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendlyConfig {
    pub label: String,
    pub position: Vec3,
    pub velocity: Vec3,
    pub class: TrackClass,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemoMetrics {
    pub case: String,
    pub seed: u64,
    pub ticks: u64,
    pub final_t: f64,
    pub truth_hostiles: usize,
    pub peak_tracks: usize,
    pub final_tracks: usize,
    pub matched_tracks: usize,
    pub false_tracks: usize,
    pub missed_truth: usize,
    /// matched / final non-friendly tracks
    pub track_purity: f64,
    /// matched / truth hostiles
    pub track_completeness: f64,
    pub false_track_rate: f64,
    pub missed_truth_rate: f64,
    pub position_rmse_m: f64,
    pub time_to_first_track_s: Option<f64>,
    pub time_to_first_recommend_s: Option<f64>,
    pub recommendations_issued: usize,
    pub multi_sensor_tracks: usize,
    pub deterministic_ok: bool,
    pub passed: bool,
    /// Peak |Δheading| (rad) between ticks among hostile air truth.
    #[serde(default)]
    pub max_heading_delta_rad: f64,
    /// 95th-percentile |Δv|/dt (m/s²) among hostile air truth.
    #[serde(default)]
    pub p95_accel_mps2: f64,
    #[serde(default)]
    pub smoothness_violations: usize,
    #[serde(default)]
    pub kinematics_ok: bool,
    #[serde(default)]
    pub fratricide_violations: usize,
    #[serde(default)]
    pub safety_violations: usize,
    /// Counts by RecommendedAction snake_case name.
    #[serde(default)]
    pub recommendation_mix: std::collections::BTreeMap<String, usize>,
    #[serde(default)]
    pub peak_detections: usize,
    #[serde(default)]
    pub scenario_class: Option<String>,
}

/// Compact snapshot for golden / bit-identical replay checks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoldenSnapshot {
    pub seed: u64,
    pub tick: u64,
    pub t: f64,
    pub truth: Vec<TruthEntity>,
    pub detections: Vec<Detection>,
    pub tracks: Vec<Track>,
}

/// Deterministic UUID stream for sim / fusion (not RFC-random).
#[derive(Debug, Clone)]
pub struct IdGen {
    stream: u64,
    counter: u64,
}

impl IdGen {
    pub fn new(seed: u64, stream: u64) -> Self {
        Self {
            stream: seed ^ stream.wrapping_mul(0x9E37_79B9_7F4A_7C15),
            counter: 0,
        }
    }

    pub fn next(&mut self) -> Uuid {
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&self.stream.to_be_bytes());
        bytes[8..16].copy_from_slice(&self.counter.to_be_bytes());
        self.counter = self.counter.wrapping_add(1);
        Uuid::from_bytes(bytes)
    }
}

pub fn now_utc() -> DateTime<Utc> {
    Utc::now()
}
