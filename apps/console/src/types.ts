export type Vec3 = { x: number; y: number; z: number }

export type SensorKind = 'radar' | 'rf' | 'eo_ir' | 'adsb' | 'acoustic'
export type Affiliation = 'hostile' | 'friendly' | 'unknown' | 'neutral'
export type TrackClass =
  | 'fixed_wing_uav'
  | 'multirotor'
  | 'swarm_member'
  | 'fiber_optic_uas'
  | 'manned'
  | 'bird'
  | 'unknown'

export type TrackStatus = 'tentative' | 'confirmed' | 'coasting' | 'dropped'
export type ZoneState = 'outside' | 'watch' | 'warning' | 'defended'
export type IdentityBasis =
  | 'adsb'
  | 'rf_signature'
  | 'eo_confirmed'
  | 'operator_labeled'
  | 'unknown'

export type RecommendedAction =
  | 'cue_eo'
  | 'alert_sector'
  | 'request_jammer_authorization'
  | 'evacuate_pad'
  | 'hand_off_higher_echelon'
  | 'engage_kinetic'
  | 'maintain_watch'

export type RecommendationStatus = 'open' | 'accepted' | 'rejected' | 'deferred'
export type OperatorDisposition = 'accepted' | 'rejected' | 'deferred'
export type Criticality = 'soft' | 'mission_critical'
export type RecommendedBy = 'rules' | 'fusion' | 'operator_assist'
export type OperatorActor = 'operator'
export type DispositionReasonCode =
  | 'approved_best_option'
  | 'insufficient_confidence'
  | 'false_positive_suspected'
  | 'waiting_for_higher'
  | 'training_only'
  | 'other'
export type EffectorKind = 'jammer' | 'kinetic'
export type DefeatCause = 'kinetic' | 'jamming'

export type EvidenceItem = { kind: string; value: string }

export type DefeatEvent = {
  t: number
  truth_id: string
  label: string
  cause: DefeatCause
  rf_dark?: boolean
  note?: string
}

export type Detection = {
  id: string
  sensor_id: string
  sensor_kind: SensorKind
  t: number
  position: Vec3
  affiliation: Affiliation
  rf_dark?: boolean
}

export type Track = {
  id: string
  t: number
  position: Vec3
  velocity: Vec3
  class_hypothesis: TrackClass
  class_confidence: number
  affiliation: Affiliation
  threat_score: number
  sensor_provenance: SensorKind[]
  hit_count: number
  coast_ticks: number
  age_s: number
  rf_dark?: boolean
  track_status?: TrackStatus
  last_update_t?: number
  nearest_asset_id?: string | null
  zone_state?: ZoneState
  identity_basis?: IdentityBasis
  /** Seconds to critical asset when closing; omitted/null if outbound. */
  eta_s?: number | null
}

export type Recommendation = {
  id: string
  track_id: string
  t: number
  priority: number
  threat_score: number
  action: RecommendedAction
  title: string
  rationale: string[]
  confidence: number
  uncertainty: string
  status?: RecommendationStatus
  disposed_at?: number | null
  criticality?: Criticality
  expected_effect?: string
  recommended_by?: RecommendedBy
  evidence?: EvidenceItem[]
  blocked_by?: string[]
  requires_confirmation?: boolean
  eta_s?: number | null
  rank_rationale?: string
}

export type OperatorEvent = {
  t: number
  track_id: string
  recommendation_id: string
  action: RecommendedAction
  disposition: OperatorDisposition
  note?: string
  actor?: OperatorActor
  reason_code?: DispositionReasonCode | null
}

export type OperatorState = {
  alerted_zone_ids: string[]
  jammer_auth_track_ids: string[]
  evacuated_pad: boolean
  handed_off_track_ids: string[]
  neutralized_truth_ids?: string[]
}

export type EffectorStatus = {
  id: string
  kind: EffectorKind
  position: Vec3
  range_m: number
  active: boolean
  tasked_track_id: string | null
  cooldown_remaining_s: number
  last_result?: string | null
}

export type SensorStatus = {
  id: string
  kind: SensorKind
  position: Vec3
  range_m: number
  healthy: boolean
  tasked_track_id: string | null
}

export type Zone = {
  id: string
  name: string
  kind: string
  center: Vec3
  radius_m: number
}

export type TruthEntity = {
  id: string
  label: string
  role: string
  position: Vec3
  velocity: Vec3
  affiliation: Affiliation
  rf_dark?: boolean
  jammed?: boolean
  neutralized?: boolean
}

export type AirPicture = {
  t: number
  tick: number
  running: boolean
  speed: number
  seed: number
  scenario_id: string
  detections: Detection[]
  tracks: Track[]
  recommendations: Recommendation[]
  sensors: SensorStatus[]
  zones: Zone[]
  truth: TruthEntity[]
  operator_events?: OperatorEvent[]
  operator_state?: OperatorState
  effectors?: EffectorStatus[]
  defeat_events?: DefeatEvent[]
}

export type ServerMessage =
  | { type: 'snapshot' } & AirPicture
  | { type: 'info'; message: string }
  | { type: 'error'; message: string }

export type ClientCommand =
  | { type: 'start' }
  | { type: 'pause' }
  | { type: 'set_speed'; speed: number }
  | { type: 'reset'; seed?: number }
  | { type: 'cue_eo'; track_id: string }
  | { type: 'fail_sensor'; sensor_id: string }
  | { type: 'restore_sensor'; sensor_id: string }
  | {
      type: 'dispose_recommendation'
      recommendation_id: string
      disposition: OperatorDisposition
      actor?: OperatorActor
      reason_code?: DispositionReasonCode
    }
