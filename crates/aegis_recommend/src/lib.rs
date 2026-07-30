use aegis_schema::{
    Affiliation, Criticality, DispositionReasonCode, EvidenceItem, OperatorActor,
    OperatorDisposition, OperatorEvent, OperatorState, Recommendation, RecommendationStatus,
    RecommendedAction, RecommendedBy, SensorKind, Track, TrackClass, Vec3, Zone, ZoneKind,
    ZoneState,
};
use std::collections::HashMap;
use uuid::Uuid;

const REJECT_COOLDOWN_S: f64 = 30.0;
const DEFER_COOLDOWN_S: f64 = 10.0;
const MAX_EVENTS: usize = 40;
/// Soft-first: when ETA exceeds this, prefer cue/alert over jammer/kinetic (unless imminent).
const ETA_SOFT_FIRST_S: f64 = 45.0;
/// Cap concurrent open jammer/kinetic recommendations — conserve scarce effectors.
const MAX_OPEN_MISSION_CRITICAL: usize = 1;
/// Closing-speed floor (m/s) to treat a track as inbound for ETA.
const CLOSING_EPS_MPS: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ActionKey {
    track_id: Uuid,
    action: RecommendedAction,
}

#[derive(Debug, Clone)]
struct Suppression {
    until_t: f64,
}

/// Sim-side effect requested when operator Accepts a recommendation.
#[derive(Debug, Clone)]
pub enum AcceptEffect {
    CueEo { track_id: Uuid },
    AlertSector,
    ActivateJammer { track_id: Uuid },
    EvacuatePad,
    HandOff { track_id: Uuid },
    EngageKinetic { track_id: Uuid },
    None,
}

#[derive(Debug, Default)]
pub struct RecommendEngine {
    last_by_track: HashMap<Uuid, Recommendation>,
    by_id: HashMap<Uuid, Recommendation>,
    suppress: HashMap<ActionKey, Suppression>,
    accepted: HashMap<ActionKey, RecommendationStatus>,
    events: Vec<OperatorEvent>,
    state: OperatorState,
    issued: usize,
    first_recommend_t: Option<f64>,
}

impl RecommendEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.last_by_track.clear();
        self.by_id.clear();
        self.suppress.clear();
        self.accepted.clear();
        self.events.clear();
        self.state = OperatorState::default();
        self.issued = 0;
        self.first_recommend_t = None;
    }

    pub fn issued_count(&self) -> usize {
        self.issued
    }

    pub fn first_recommend_t(&self) -> Option<f64> {
        self.first_recommend_t
    }

    pub fn operator_events(&self) -> &[OperatorEvent] {
        &self.events
    }

    pub fn operator_state(&self) -> &OperatorState {
        &self.state
    }

    pub fn dispose(
        &mut self,
        t: f64,
        recommendation_id: Uuid,
        disposition: OperatorDisposition,
        actor: OperatorActor,
        reason_code: Option<DispositionReasonCode>,
    ) -> Option<(OperatorEvent, AcceptEffect)> {
        let rec = self.by_id.get(&recommendation_id)?.clone();
        if rec.status != RecommendationStatus::Open {
            return None;
        }

        let status = disposition.to_status();
        let key = ActionKey {
            track_id: rec.track_id,
            action: rec.action,
        };

        let (note, effect) = match disposition {
            OperatorDisposition::Accepted => {
                self.accepted.insert(key, RecommendationStatus::Accepted);
                self.suppress.remove(&key);
                let note = rec.expected_effect.clone();
                let effect = match rec.action {
                    RecommendedAction::CueEo => AcceptEffect::CueEo {
                        track_id: rec.track_id,
                    },
                    RecommendedAction::AlertSector => AcceptEffect::AlertSector,
                    RecommendedAction::RequestJammerAuthorization => {
                        if !self.state.jammer_auth_track_ids.contains(&rec.track_id) {
                            self.state.jammer_auth_track_ids.push(rec.track_id);
                        }
                        AcceptEffect::ActivateJammer {
                            track_id: rec.track_id,
                        }
                    }
                    RecommendedAction::EvacuatePad => {
                        self.state.evacuated_pad = true;
                        AcceptEffect::EvacuatePad
                    }
                    RecommendedAction::HandOffHigherEchelon => {
                        if !self.state.handed_off_track_ids.contains(&rec.track_id) {
                            self.state.handed_off_track_ids.push(rec.track_id);
                        }
                        AcceptEffect::HandOff {
                            track_id: rec.track_id,
                        }
                    }
                    RecommendedAction::EngageKinetic => AcceptEffect::EngageKinetic {
                        track_id: rec.track_id,
                    },
                    RecommendedAction::MaintainWatch => AcceptEffect::None,
                };
                (note, effect)
            }
            OperatorDisposition::Rejected => {
                self.suppress.insert(
                    key,
                    Suppression {
                        until_t: t + REJECT_COOLDOWN_S,
                    },
                );
                self.accepted.remove(&key);
                (
                    format!("Rejected — suppressed {REJECT_COOLDOWN_S:.0}s"),
                    AcceptEffect::None,
                )
            }
            OperatorDisposition::Deferred => {
                self.suppress.insert(
                    key,
                    Suppression {
                        until_t: t + DEFER_COOLDOWN_S,
                    },
                );
                (
                    format!("Deferred — snoozed {DEFER_COOLDOWN_S:.0}s"),
                    AcceptEffect::None,
                )
            }
        };

        let mut updated = rec.clone();
        updated.status = status;
        updated.disposed_at = Some(t);
        updated.blocked_by.clear();
        self.by_id.insert(recommendation_id, updated.clone());
        self.last_by_track.insert(rec.track_id, updated);

        let event = OperatorEvent {
            t,
            track_id: rec.track_id,
            recommendation_id,
            action: rec.action,
            disposition,
            note,
            actor,
            reason_code,
        };
        self.push_event(event.clone());
        Some((event, effect))
    }

    pub fn annotate_effect_result(&mut self, note: impl Into<String>) {
        if let Some(last) = self.events.last_mut() {
            let extra = note.into();
            if last.note.is_empty() {
                last.note = extra;
            } else if !extra.is_empty() {
                last.note = format!("{} — {}", last.note, extra);
            }
        }
    }

    pub fn mark_zone_alerted(&mut self, zone_id: impl Into<String>) {
        let id = zone_id.into();
        if !self.state.alerted_zone_ids.contains(&id) {
            self.state.alerted_zone_ids.push(id);
        }
    }

    pub fn mark_neutralized(&mut self, truth_id: Uuid) {
        if !self.state.neutralized_truth_ids.contains(&truth_id) {
            self.state.neutralized_truth_ids.push(truth_id);
        }
    }

    fn push_event(&mut self, event: OperatorEvent) {
        self.events.push(event);
        if self.events.len() > MAX_EVENTS {
            let drain = self.events.len() - MAX_EVENTS;
            self.events.drain(0..drain);
        }
    }

    fn suppressed(&self, t: f64, track_id: Uuid, action: RecommendedAction) -> bool {
        let key = ActionKey { track_id, action };
        self.suppress
            .get(&key)
            .map(|s| t < s.until_t)
            .unwrap_or(false)
    }

    pub fn evaluate(
        &mut self,
        t: f64,
        tracks: &mut [Track],
        zones: &[Zone],
    ) -> Vec<Recommendation> {
        // Drop expired suppressions
        self.suppress.retain(|_, s| t < s.until_t);

        let asset_zone = zones.iter().find(|z| z.kind == ZoneKind::CriticalAsset);
        let asset = asset_zone.map(|z| z.center).unwrap_or(Vec3::zero());
        let keep_out = zones
            .iter()
            .find(|z| z.kind == ZoneKind::KeepOut || z.kind == ZoneKind::NoFly);

        for tr in tracks.iter_mut() {
            enrich_track_zones(tr, zones, asset_zone);
        }

        let hostile_pos: Vec<Vec3> = tracks
            .iter()
            .filter(|tr| tr.affiliation != Affiliation::Friendly)
            .map(|tr| tr.position)
            .collect();

        for tr in tracks.iter_mut() {
            if tr.affiliation == Affiliation::Friendly {
                tr.threat_score = 0.0;
                tr.eta_s = None;
                continue;
            }

            let dist = tr.position.distance(&asset);
            let speed = tr.velocity.magnitude_xy();
            let closing = closing_speed(&tr.position, &tr.velocity, &asset);
            tr.eta_s = eta_seconds(dist, closing);
            let zone_breach = keep_out
                .map(|z| tr.position.distance_xy(&z.center) < z.radius_m)
                .unwrap_or(false);
            let multi = tr.sensor_provenance.len();
            let neighbors = hostile_pos
                .iter()
                .filter(|p| p.distance_xy(&tr.position) < 250.0)
                .count()
                .saturating_sub(1);

            let mut score = 0.0;
            score += (1.0 - (dist / 5000.0).clamp(0.0, 1.0)) * 28.0;
            score += closing.max(0.0).min(80.0) * 0.25;
            score += (speed / 40.0).clamp(0.0, 1.0) * 8.0;
            // Impact / trajectory: tighter ETA → higher priority; outbound demoted.
            if let Some(eta) = tr.eta_s {
                score += (1.0 - (eta / 90.0).clamp(0.0, 1.0)) * 30.0;
            } else {
                score *= 0.4;
                score = score.min(32.0);
            }
            if zone_breach {
                score += 22.0;
            }
            score += (multi.saturating_sub(1) as f64) * 8.0;
            score += (neighbors.min(6) as f64) * 4.0;
            if matches!(
                tr.class_hypothesis,
                TrackClass::SwarmMember | TrackClass::Multirotor | TrackClass::FiberOpticUas
            ) {
                score += 6.0;
            }
            if tr.rf_dark || tr.class_hypothesis == TrackClass::FiberOpticUas {
                score += 5.0;
            }
            if tr.coast_ticks > 10 {
                score *= 0.75;
            }
            tr.threat_score = score.clamp(0.0, 100.0);
        }

        let mut ranked: Vec<&Track> = tracks
            .iter()
            .filter(|tr| tr.affiliation != Affiliation::Friendly && tr.threat_score >= 20.0)
            .collect();
        ranked.sort_by(|a, b| {
            b.threat_score
                .partial_cmp(&a.threat_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut recs = Vec::new();
        let mut open_mission_critical = 0usize;
        for tr in ranked.iter().take(8) {
            // Safety: never escalate friendlies to mission-critical.
            if tr.affiliation == Affiliation::Friendly {
                continue;
            }

            let closing = closing_speed(&tr.position, &tr.velocity, &asset);
            let (mut action, mut title, mut rationale, confidence, uncertainty) =
                decide_action(tr, &asset, keep_out);

            if is_mission_critical(action) && tr.affiliation == Affiliation::Friendly {
                continue;
            }

            // Cap concurrent open jammer/kinetic — prefer one high-priority effector over many mid-tier.
            let provisional_key = ActionKey {
                track_id: tr.id,
                action,
            };
            let already_resolved = self.accepted.contains_key(&provisional_key);
            if is_mission_critical(action)
                && !already_resolved
                && open_mission_critical >= MAX_OPEN_MISSION_CRITICAL
            {
                let soft = conserve_soft_alternative(tr, keep_out);
                action = soft.0;
                title = soft.1;
                rationale.push(
                    "Effector capacity conserved — soft action while higher-priority track holds jammer/kinetic slot"
                        .into(),
                );
            }

            if self.suppressed(t, tr.id, action) {
                continue;
            }

            let prev = self.last_by_track.get(&tr.id);
            let id = prev
                .filter(|p| p.action == action)
                .map(|p| p.id)
                .unwrap_or_else(Uuid::new_v4);
            let action_changed = prev.map(|p| p.action != action).unwrap_or(true);

            let key = ActionKey {
                track_id: tr.id,
                action,
            };
            let (status, disposed_at) = if let Some(s) = self.accepted.get(&key) {
                (*s, prev.and_then(|p| p.disposed_at).or(Some(t)))
            } else if let Some(p) =
                prev.filter(|p| p.id == id && p.status != RecommendationStatus::Open)
            {
                (p.status, p.disposed_at)
            } else {
                (RecommendationStatus::Open, None)
            };

            let criticality = if is_mission_critical(action) {
                Criticality::MissionCritical
            } else {
                Criticality::Soft
            };
            let requires_confirmation = criticality == Criticality::MissionCritical;
            let mut blocked_by = Vec::new();
            if requires_confirmation && status == RecommendationStatus::Open {
                blocked_by.push("operator_confirmation".into());
            }
            if is_mission_critical(action) && status == RecommendationStatus::Open {
                open_mission_critical += 1;
            }

            let rank = build_rank_rationale(tr, closing);
            let rec = Recommendation {
                id,
                track_id: tr.id,
                t,
                priority: (recs.len() as u32) + 1,
                threat_score: tr.threat_score,
                action,
                title,
                rationale,
                confidence,
                uncertainty,
                status,
                disposed_at,
                criticality,
                expected_effect: expected_effect(action, tr),
                recommended_by: RecommendedBy::Rules,
                evidence: build_evidence(tr, keep_out, closing),
                blocked_by,
                requires_confirmation,
                eta_s: tr.eta_s,
                rank_rationale: rank,
            };
            if self.first_recommend_t.is_none() {
                self.first_recommend_t = Some(t);
            }
            if action_changed && status == RecommendationStatus::Open {
                self.issued += 1;
            }
            self.last_by_track.insert(tr.id, rec.clone());
            self.by_id.insert(rec.id, rec.clone());
            recs.push(rec);
            if recs.len() >= 5 {
                break;
            }
        }
        recs
    }
}

pub fn is_mission_critical(action: RecommendedAction) -> bool {
    matches!(
        action,
        RecommendedAction::RequestJammerAuthorization | RecommendedAction::EngageKinetic
    )
}

fn enrich_track_zones(tr: &mut Track, zones: &[Zone], asset_zone: Option<&Zone>) {
    if let Some(az) = asset_zone {
        tr.nearest_asset_id = Some(az.id.clone());
    }
    let in_asset = asset_zone
        .map(|z| tr.position.distance_xy(&z.center) < z.radius_m)
        .unwrap_or(false);
    let in_nofly = zones
        .iter()
        .filter(|z| z.kind == ZoneKind::NoFly)
        .any(|z| tr.position.distance_xy(&z.center) < z.radius_m);
    let in_keep = zones
        .iter()
        .filter(|z| z.kind == ZoneKind::KeepOut)
        .any(|z| tr.position.distance_xy(&z.center) < z.radius_m);
    tr.zone_state = if in_asset {
        ZoneState::Defended
    } else if in_nofly {
        ZoneState::Warning
    } else if in_keep {
        ZoneState::Watch
    } else {
        ZoneState::Outside
    };
}

fn expected_effect(action: RecommendedAction, tr: &Track) -> String {
    match action {
        RecommendedAction::CueEo => {
            "Task EO/IR on this track — improve class confidence; conserve jammer/kinetic (no kill)"
                .into()
        }
        RecommendedAction::AlertSector => {
            "Mark keep-out / no-fly as alerted — soft posture while ETA allows".into()
        }
        RecommendedAction::RequestJammerAuthorization => {
            if tr.rf_dark || tr.class_hypothesis == TrackClass::FiberOpticUas {
                "Authorize jammer — likely ineffective on RF-dark/fiber".into()
            } else {
                "Authorize jammer dwell — degrade RF C2 on this priority track".into()
            }
        }
        RecommendedAction::EvacuatePad => "Set pad evacuated status for base ops".into(),
        RecommendedAction::HandOffHigherEchelon => {
            "Record hand-off to higher echelon (no local effector)".into()
        }
        RecommendedAction::EngageKinetic => {
            "Authorize sim kinetic effector — Pk/HIT after TOF (operator-authorized only)".into()
        }
        RecommendedAction::MaintainWatch => {
            "Continue fused tracking — hold effectors; conserve capacity".into()
        }
    }
}

fn build_evidence(tr: &Track, keep_out: Option<&Zone>, closing: f64) -> Vec<EvidenceItem> {
    let mut ev = Vec::new();
    match tr.eta_s {
        Some(eta) => ev.push(EvidenceItem {
            kind: "eta".into(),
            value: format!("{eta:.0}s"),
        }),
        None => ev.push(EvidenceItem {
            kind: "eta".into(),
            value: "outbound".into(),
        }),
    }
    ev.push(EvidenceItem {
        kind: "closing".into(),
        value: format!("{closing:.0}"),
    });
    if !tr.sensor_provenance.is_empty() {
        let modes: Vec<_> = tr
            .sensor_provenance
            .iter()
            .map(|k| format!("{k:?}").to_ascii_lowercase())
            .collect();
        ev.push(EvidenceItem {
            kind: "provenance".into(),
            value: modes.join("+"),
        });
    }
    ev.push(EvidenceItem {
        kind: "class".into(),
        value: format!(
            "{:?}@{:.0}%",
            tr.class_hypothesis,
            tr.class_confidence * 100.0
        ),
    });
    if tr.rf_dark {
        ev.push(EvidenceItem {
            kind: "rf_dark".into(),
            value: "true".into(),
        });
    }
    if let Some(z) = keep_out {
        if tr.position.distance_xy(&z.center) < z.radius_m {
            ev.push(EvidenceItem {
                kind: "zone".into(),
                value: format!("inside_{}", z.id),
            });
        }
    }
    ev.push(EvidenceItem {
        kind: "zone_state".into(),
        value: format!("{:?}", tr.zone_state).to_ascii_lowercase(),
    });
    ev
}

fn closing_speed(pos: &Vec3, vel: &Vec3, target: &Vec3) -> f64 {
    let dx = target.x - pos.x;
    let dy = target.y - pos.y;
    let dist = (dx * dx + dy * dy).sqrt().max(1.0);
    (vel.x * dx + vel.y * dy) / dist
}

fn eta_seconds(dist: f64, closing: f64) -> Option<f64> {
    if closing > CLOSING_EPS_MPS {
        Some((dist / closing).max(0.0))
    } else {
        None
    }
}

fn build_rank_rationale(tr: &Track, closing: f64) -> String {
    let eta_part = match tr.eta_s {
        Some(e) => format!("ETA {e:.0}s"),
        None => "outbound / no ETA".into(),
    };
    let close_part = if closing > CLOSING_EPS_MPS {
        format!("closing {closing:.0} m/s")
    } else if closing < -CLOSING_EPS_MPS {
        format!("opening {closing:.0} m/s")
    } else {
        "not closing".into()
    };
    let zone_part = format!("zone {:?}", tr.zone_state).to_ascii_lowercase();
    format!(
        "{eta_part} · {close_part} · {zone_part} · impact {:.0}/100",
        tr.threat_score
    )
}

fn eta_comfortable(tr: &Track) -> bool {
    match tr.eta_s {
        Some(e) => e > ETA_SOFT_FIRST_S,
        None => true, // outbound — no rush to expend effectors
    }
}

fn conserve_soft_alternative(tr: &Track, keep_out: Option<&Zone>) -> (RecommendedAction, String) {
    let zone_breach = keep_out
        .map(|z| tr.position.distance_xy(&z.center) < z.radius_m)
        .unwrap_or(false);
    if zone_breach {
        (
            RecommendedAction::AlertSector,
            "Alert sector — conserve jammer/kinetic for higher-priority track".into(),
        )
    } else if tr.sensor_provenance.len() < 2 || tr.class_confidence < 0.5 {
        (
            RecommendedAction::CueEo,
            format!(
                "Cue EO/IR on {} — conserve effectors for tighter ETA",
                short_id(tr.id)
            ),
        )
    } else {
        (
            RecommendedAction::MaintainWatch,
            "Maintain watch — conserve jammer/kinetic capacity".into(),
        )
    }
}

fn decide_action(
    tr: &Track,
    asset: &Vec3,
    keep_out: Option<&Zone>,
) -> (RecommendedAction, String, Vec<String>, f64, String) {
    let dist = tr.position.distance(asset);
    let multi = tr.sensor_provenance.len();
    let zone_breach = keep_out
        .map(|z| tr.position.distance_xy(&z.center) < z.radius_m)
        .unwrap_or(false);
    let fiber = tr.rf_dark || tr.class_hypothesis == TrackClass::FiberOpticUas;
    let has_eo = tr
        .sensor_provenance
        .iter()
        .any(|k| matches!(k, SensorKind::EoIr));
    let has_radar = tr
        .sensor_provenance
        .iter()
        .any(|k| matches!(k, SensorKind::Radar));
    let soft_first = eta_comfortable(tr) && dist >= 700.0;

    let mut rationale = Vec::new();
    rationale.push(format!(
        "Threat score {:.0}/100; {:.0} m from critical asset",
        tr.threat_score, dist
    ));
    match tr.eta_s {
        Some(eta) => rationale.push(format!("ETA to asset ≈ {eta:.0}s")),
        None => rationale.push("Outbound / not closing — demoted for effector thrift".into()),
    }
    if fiber {
        rationale.push("RF-dark / fiber-optic class — RF soft-kill likely ineffective".into());
    }
    if multi >= 2 {
        rationale.push(format!("Corroborated by {} sensor modalities", multi));
    } else {
        rationale.push("Single-modality track — classification uncertain".into());
    }
    if zone_breach {
        rationale.push("Inside keep-out / no-fly sector".into());
    }
    if tr.hit_count < 4 {
        rationale.push("Young track — limited hit history".into());
    }
    if soft_first {
        rationale.push(format!(
            "ETA > {ETA_SOFT_FIRST_S:.0}s — soft-first to conserve jammer/kinetic"
        ));
    }

    let uncertainty = if fiber {
        "RF-dark — soft-kill RF likely ineffective".into()
    } else if multi < 2 {
        "High — await EO or RF corroboration before irreversible action".into()
    } else if tr.coast_ticks > 5 {
        "Moderate — track is coasting through dropout".into()
    } else {
        "Low–moderate — kinematics and sensors agree".into()
    };

    // Fiber doctrine: cue / alert / engage (kinetic) / hand-off — not jammer-first.
    if fiber {
        if !has_eo || tr.class_confidence < 0.75 {
            return (
                RecommendedAction::CueEo,
                format!("Cue EO/IR on RF-dark track {}", short_id(tr.id)),
                rationale,
                0.8,
                uncertainty,
            );
        }
        if !soft_first && dist < 1600.0 && tr.threat_score > 55.0 && has_radar && has_eo {
            rationale.push("Corroborated RF-dark threat — kinetic engage authorized path".into());
            return (
                RecommendedAction::EngageKinetic,
                format!("Engage kinetic on fiber track {}", short_id(tr.id)),
                rationale,
                0.68,
                uncertainty,
            );
        }
        if zone_breach || dist < 2200.0 {
            return (
                RecommendedAction::AlertSector,
                "Alert sector — RF-dark inbound; non-RF effects preferred".into(),
                rationale,
                0.76,
                uncertainty,
            );
        }
        if tr.threat_score > 60.0 {
            return (
                RecommendedAction::HandOffHigherEchelon,
                "Hand off RF-dark fiber track to higher echelon".into(),
                rationale,
                0.7,
                uncertainty,
            );
        }
        return (
            RecommendedAction::MaintainWatch,
            "Maintain watch on RF-dark track — continue acoustic/radar fusion".into(),
            rationale,
            0.65,
            uncertainty,
        );
    }

    // RF hostiles: jammer when deep in keep-out; kinetic when high threat + multi-sensor.
    // Soft-first when ETA is comfortable — conserve scarce effectors.
    if !soft_first && zone_breach && dist < 900.0 {
        return (
            RecommendedAction::RequestJammerAuthorization,
            format!("Authorize soft-kill window on {}", short_id(tr.id)),
            rationale,
            0.62,
            uncertainty,
        );
    }
    if !soft_first
        && dist < 1200.0
        && tr.threat_score > 65.0
        && has_radar
        && (has_eo || multi >= 2)
        && tr.class_confidence >= 0.5
    {
        rationale.push("High-confidence inbound — kinetic engage available".into());
        return (
            RecommendedAction::EngageKinetic,
            format!("Engage kinetic on {}", short_id(tr.id)),
            rationale,
            0.7,
            uncertainty,
        );
    }
    if dist < 1400.0 && tr.threat_score > 55.0 {
        return (
            RecommendedAction::EvacuatePad,
            "Evacuate flightline / pad under inbound swarm vector".into(),
            rationale,
            0.7,
            uncertainty,
        );
    }
    if multi < 2 || tr.class_confidence < 0.5 {
        return (
            RecommendedAction::CueEo,
            format!("Cue EO/IR on track {}", short_id(tr.id)),
            rationale,
            0.78,
            uncertainty,
        );
    }
    if tr.threat_score > 70.0 {
        return (
            RecommendedAction::HandOffHigherEchelon,
            "Hand off priority tracks to higher echelon".into(),
            rationale,
            0.66,
            uncertainty,
        );
    }
    if zone_breach || dist < 2200.0 {
        return (
            RecommendedAction::AlertSector,
            "Alert threatened sector / base defense net".into(),
            rationale,
            0.74,
            uncertainty,
        );
    }
    (
        RecommendedAction::MaintainWatch,
        "Maintain watch — continue fused tracking".into(),
        rationale,
        0.6,
        uncertainty,
    )
}

fn short_id(id: Uuid) -> String {
    id.to_string()[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_track(id: Uuid, fiber: bool) -> Track {
        Track {
            id,
            t: 10.0,
            position: Vec3::new(800.0, 0.0, 100.0),
            velocity: Vec3::new(-40.0, 0.0, 0.0),
            covariance_trace: 10.0,
            class_hypothesis: if fiber {
                TrackClass::FiberOpticUas
            } else {
                TrackClass::SwarmMember
            },
            class_confidence: 0.7,
            affiliation: Affiliation::Hostile,
            threat_score: 0.0,
            sensor_provenance: if fiber {
                vec![SensorKind::Radar, SensorKind::Acoustic]
            } else {
                vec![SensorKind::Radar, SensorKind::Rf]
            },
            hit_count: 12,
            coast_ticks: 0,
            age_s: 8.0,
            rf_dark: fiber,
            track_status: Default::default(),
            last_update_t: 10.0,
            nearest_asset_id: None,
            zone_state: Default::default(),
            identity_basis: Default::default(),
            eta_s: None,
        }
    }

    fn zones() -> Vec<Zone> {
        vec![
            Zone {
                id: "asset".into(),
                name: "TOC".into(),
                kind: ZoneKind::CriticalAsset,
                center: Vec3::zero(),
                radius_m: 150.0,
            },
            Zone {
                id: "ko".into(),
                name: "KO".into(),
                kind: ZoneKind::KeepOut,
                center: Vec3::zero(),
                radius_m: 2200.0,
            },
        ]
    }

    #[test]
    fn scores_inbound_hostile() {
        let mut eng = RecommendEngine::new();
        let mut tracks = vec![sample_track(Uuid::new_v4(), false)];
        let recs = eng.evaluate(10.0, &mut tracks, &zones());
        assert!(!recs.is_empty());
        assert!(tracks[0].threat_score > 40.0);
    }

    #[test]
    fn fiber_track_avoids_jammer_first() {
        let mut eng = RecommendEngine::new();
        let mut tracks = vec![sample_track(Uuid::new_v4(), true)];
        tracks[0].position = Vec3::new(600.0, 0.0, 100.0);
        tracks[0].class_confidence = 0.78;
        let recs = eng.evaluate(10.0, &mut tracks, &zones());
        assert!(!recs.is_empty());
        assert_ne!(
            recs[0].action,
            RecommendedAction::RequestJammerAuthorization
        );
        assert!(recs[0].uncertainty.contains("RF-dark"));
    }

    #[test]
    fn reject_suppresses_same_action() {
        let mut eng = RecommendEngine::new();
        let id = Uuid::new_v4();
        let mut tracks = vec![sample_track(id, false)];
        let recs = eng.evaluate(10.0, &mut tracks, &zones());
        let rec = recs[0].clone();
        let action = rec.action;
        eng.dispose(
            10.0,
            rec.id,
            OperatorDisposition::Rejected,
            OperatorActor::Operator,
            Some(DispositionReasonCode::FalsePositiveSuspected),
        )
        .expect("dispose");
        let recs2 = eng.evaluate(12.0, &mut tracks, &zones());
        assert!(
            recs2
                .iter()
                .all(|r| !(r.track_id == id && r.action == action)),
            "rejected action should be suppressed"
        );
    }

    #[test]
    fn accept_cue_eo_returns_effect() {
        let mut eng = RecommendEngine::new();
        let id = Uuid::new_v4();
        let mut tracks = vec![sample_track(id, false)];
        tracks[0].sensor_provenance = vec![SensorKind::Radar];
        tracks[0].class_confidence = 0.3;
        tracks[0].position = Vec3::new(2500.0, 0.0, 100.0);
        let recs = eng.evaluate(10.0, &mut tracks, &zones());
        let cue = recs
            .iter()
            .find(|r| r.action == RecommendedAction::CueEo)
            .expect("cue eo rec");
        assert!(!cue.requires_confirmation);
        assert_eq!(cue.criticality, Criticality::Soft);
        assert!(!cue.expected_effect.is_empty());
        let (_ev, effect) = eng
            .dispose(
                10.0,
                cue.id,
                OperatorDisposition::Accepted,
                OperatorActor::Operator,
                Some(DispositionReasonCode::ApprovedBestOption),
            )
            .expect("accept");
        match effect {
            AcceptEffect::CueEo { track_id } => assert_eq!(track_id, id),
            other => panic!("expected CueEo, got {other:?}"),
        }
        assert!(!eng.operator_events().is_empty());
    }

    #[test]
    fn friendly_adsb_never_mission_critical() {
        let mut eng = RecommendEngine::new();
        let id = Uuid::new_v4();
        let mut tracks = vec![Track {
            affiliation: Affiliation::Friendly,
            class_hypothesis: TrackClass::Manned,
            sensor_provenance: vec![SensorKind::Adsb],
            identity_basis: aegis_schema::IdentityBasis::Adsb,
            position: Vec3::new(400.0, 0.0, 200.0),
            threat_score: 99.0,
            ..sample_track(id, false)
        }];
        let recs = eng.evaluate(10.0, &mut tracks, &zones());
        assert!(
            recs.iter().all(|r| !is_mission_critical(r.action)),
            "friendly must not get jammer/kinetic"
        );
    }

    #[test]
    fn mission_critical_requires_confirmation_flag() {
        let mut eng = RecommendEngine::new();
        let mut tracks = vec![sample_track(Uuid::new_v4(), false)];
        tracks[0].position = Vec3::new(500.0, 0.0, 80.0);
        tracks[0].class_confidence = 0.8;
        tracks[0].sensor_provenance = vec![SensorKind::Radar, SensorKind::EoIr, SensorKind::Rf];
        let recs = eng.evaluate(10.0, &mut tracks, &zones());
        let critical = recs.iter().find(|r| is_mission_critical(r.action));
        if let Some(c) = critical {
            assert!(c.requires_confirmation);
            assert_eq!(c.criticality, Criticality::MissionCritical);
            assert!(c.blocked_by.iter().any(|b| b == "operator_confirmation"));
            assert!(c.eta_s.is_some());
            assert!(!c.rank_rationale.is_empty());
        }
    }

    #[test]
    fn tighter_eta_ranks_higher_when_inbound() {
        let mut eng = RecommendEngine::new();
        let near_id = Uuid::new_v4();
        let far_id = Uuid::new_v4();
        let mut near = sample_track(near_id, false);
        near.position = Vec3::new(400.0, 0.0, 80.0);
        near.velocity = Vec3::new(-50.0, 0.0, 0.0);
        near.sensor_provenance = vec![SensorKind::Radar, SensorKind::Rf];
        let mut far = sample_track(far_id, false);
        far.position = Vec3::new(2400.0, 0.0, 80.0);
        far.velocity = Vec3::new(-20.0, 0.0, 0.0);
        far.sensor_provenance = vec![SensorKind::Radar, SensorKind::Rf];
        let mut tracks = vec![far, near];
        let recs = eng.evaluate(10.0, &mut tracks, &zones());
        let near_tr = tracks.iter().find(|t| t.id == near_id).unwrap();
        let far_tr = tracks.iter().find(|t| t.id == far_id).unwrap();
        assert!(near_tr.eta_s.is_some() && far_tr.eta_s.is_some());
        assert!(near_tr.eta_s.unwrap() < far_tr.eta_s.unwrap());
        assert!(near_tr.threat_score > far_tr.threat_score);
        let near_pri = recs
            .iter()
            .find(|r| r.track_id == near_id)
            .map(|r| r.priority);
        let far_pri = recs
            .iter()
            .find(|r| r.track_id == far_id)
            .map(|r| r.priority);
        if let (Some(np), Some(fp)) = (near_pri, far_pri) {
            assert!(
                np < fp,
                "tighter ETA should get better (lower) priority number"
            );
        }
        assert!(recs.iter().any(|r| {
            r.evidence.iter().any(|e| e.kind == "eta")
                && r.evidence.iter().any(|e| e.kind == "closing")
        }));
    }

    #[test]
    fn outbound_demoted_vs_inbound() {
        let mut eng = RecommendEngine::new();
        let inbound_id = Uuid::new_v4();
        let outbound_id = Uuid::new_v4();
        let mut inbound = sample_track(inbound_id, false);
        inbound.position = Vec3::new(1200.0, 0.0, 80.0);
        inbound.velocity = Vec3::new(-35.0, 0.0, 0.0);
        let mut outbound = sample_track(outbound_id, false);
        outbound.position = Vec3::new(1200.0, 0.0, 80.0);
        outbound.velocity = Vec3::new(35.0, 0.0, 0.0);
        let mut tracks = vec![outbound, inbound];
        eng.evaluate(10.0, &mut tracks, &zones());
        let in_tr = tracks.iter().find(|t| t.id == inbound_id).unwrap();
        let out_tr = tracks.iter().find(|t| t.id == outbound_id).unwrap();
        assert!(in_tr.eta_s.is_some());
        assert!(out_tr.eta_s.is_none());
        assert!(in_tr.threat_score > out_tr.threat_score);
    }

    #[test]
    fn soft_first_when_eta_comfortable() {
        let mut eng = RecommendEngine::new();
        let mut tracks = vec![sample_track(Uuid::new_v4(), false)];
        // Far inbound with slow closing → ETA >> soft-first threshold.
        tracks[0].position = Vec3::new(2800.0, 0.0, 100.0);
        tracks[0].velocity = Vec3::new(-25.0, 0.0, 0.0);
        tracks[0].class_confidence = 0.8;
        tracks[0].sensor_provenance = vec![SensorKind::Radar, SensorKind::EoIr, SensorKind::Rf];
        let recs = eng.evaluate(10.0, &mut tracks, &zones());
        assert!(!recs.is_empty());
        assert!(tracks[0].eta_s.unwrap() > ETA_SOFT_FIRST_S);
        assert!(
            !is_mission_critical(recs[0].action),
            "comfortable ETA should soft-first, got {:?}",
            recs[0].action
        );
        assert!(
            recs[0]
                .rationale
                .iter()
                .any(|r| r.contains("soft-first") || r.contains("conserve")),
            "expected conserve/soft-first rationale"
        );
    }

    #[test]
    fn caps_concurrent_open_mission_critical() {
        let mut eng = RecommendEngine::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let mut ta = sample_track(a, false);
        ta.position = Vec3::new(450.0, 0.0, 80.0);
        ta.velocity = Vec3::new(-45.0, 0.0, 0.0);
        ta.class_confidence = 0.85;
        ta.sensor_provenance = vec![SensorKind::Radar, SensorKind::EoIr, SensorKind::Rf];
        let mut tb = sample_track(b, false);
        tb.position = Vec3::new(480.0, 40.0, 80.0);
        tb.velocity = Vec3::new(-42.0, 0.0, 0.0);
        tb.class_confidence = 0.85;
        tb.sensor_provenance = vec![SensorKind::Radar, SensorKind::EoIr, SensorKind::Rf];
        let mut tracks = vec![ta, tb];
        let recs = eng.evaluate(10.0, &mut tracks, &zones());
        let open_critical = recs
            .iter()
            .filter(|r| r.status == RecommendationStatus::Open && is_mission_critical(r.action))
            .count();
        assert!(
            open_critical <= MAX_OPEN_MISSION_CRITICAL,
            "expected <= {MAX_OPEN_MISSION_CRITICAL} open mission-critical, got {open_critical}"
        );
    }
}
