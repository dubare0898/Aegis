//! Auto-engage policy mirroring the console Operator/Auto engage package selection,
//! plus north-star ranking / closed-loop scoring helpers.

use aegis_recommend::{is_mission_critical, AcceptEffect, RecommendEngine};
use aegis_schema::{
    Affiliation, Criticality, DefeatCause, DispositionReasonCode, EffectorKind, EffectorStatus,
    OperatorActor, OperatorDisposition, Recommendation, RecommendationStatus, RecommendedAction,
    Track, TruthEntity, Zone, ZoneKind,
};
use aegis_sim::Simulation;
use uuid::Uuid;

const MAX_BATCH_ENGAGE: usize = 5;
const HIGH_THREAT_SCORE: f64 = 48.0;

#[derive(Debug, Default)]
pub struct ClosedLoopAccum {
    pub auto_engage: bool,
    pub completeness_at_decision_horizon: Option<f64>,
    pub decision_horizon_t: Option<f64>,
    pub eta_rank_hits: usize,
    pub eta_rank_samples: usize,
    pub jammer_activations: usize,
    pub kinetic_shots: usize,
    pub jammer_on_rf_dark: usize,
    pub asset_breaches: usize,
    pub auto_accepts: usize,
    pub time_to_neutralize_high_eta_s: Option<f64>,
    /// Truth ids that were inbound (had ETA) when first seen neutralized.
    seen_neutralized: std::collections::HashSet<Uuid>,
}

impl ClosedLoopAccum {
    pub fn new(auto_engage: bool) -> Self {
        Self {
            auto_engage,
            ..Default::default()
        }
    }

    pub fn observe_ranking(&mut self, recs: &[Recommendation]) {
        let mut inbound: Vec<(u32, f64)> = recs
            .iter()
            .filter(|r| r.status == RecommendationStatus::Open)
            .filter(|r| is_mission_critical(r.action))
            .filter_map(|r| r.eta_s.map(|eta| (r.priority, eta)))
            .collect();
        if inbound.len() < 2 {
            return;
        }
        self.eta_rank_samples += 1;
        inbound.sort_by_key(|(p, _)| *p);
        let top_eta = inbound[0].1;
        let mut etas: Vec<f64> = inbound.iter().map(|(_, e)| *e).collect();
        etas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let k = 2.min(etas.len());
        let among_tightest = etas[..k]
            .iter()
            .any(|&e| (e - top_eta).abs() < 1e-6 || top_eta <= e);
        // P1 ETA is among the k tightest (allow near-tie slack).
        let kth = etas[k - 1];
        if among_tightest || top_eta <= kth * 1.15 + 5.0 {
            self.eta_rank_hits += 1;
        }
    }

    pub fn maybe_capture_horizon(&mut self, t: f64, completeness: f64, recs: &[Recommendation]) {
        if self.decision_horizon_t.is_some() {
            return;
        }
        let mc_open = recs
            .iter()
            .any(|r| r.status == RecommendationStatus::Open && is_mission_critical(r.action));
        if mc_open {
            self.decision_horizon_t = Some(t);
            self.completeness_at_decision_horizon = Some(completeness);
        }
    }

    pub fn observe_breaches(&mut self, truth: &[TruthEntity], zones: &[Zone]) {
        let Some(asset) = zones.iter().find(|z| z.kind == ZoneKind::CriticalAsset) else {
            return;
        };
        for te in truth {
            if te.affiliation != Affiliation::Hostile || te.neutralized {
                continue;
            }
            if te.position.distance_xy(&asset.center) < asset.radius_m {
                self.asset_breaches += 1;
                break; // count at most one breach tick
            }
        }
    }

    pub fn observe_defeats(&mut self, t: f64, events: &[aegis_schema::DefeatEvent]) {
        for ev in events {
            if !self.seen_neutralized.insert(ev.truth_id) {
                continue;
            }
            if self.time_to_neutralize_high_eta_s.is_none() {
                self.time_to_neutralize_high_eta_s = Some(t);
            }
            if ev.cause == DefeatCause::Jamming && ev.rf_dark {
                self.jammer_on_rf_dark += 1;
            }
        }
    }

    pub fn finish(
        &self,
        initial_hostiles: usize,
        final_truth: &[TruthEntity],
    ) -> aegis_schema::ClosedLoopMetrics {
        let neutralized = final_truth
            .iter()
            .filter(|e| e.affiliation == Affiliation::Hostile && e.neutralized)
            .count();
        let shots = self.jammer_activations + self.kinetic_shots;
        let neutralize_fraction = if initial_hostiles > 0 {
            neutralized as f64 / initial_hostiles as f64
        } else {
            0.0
        };
        let neutralize_per = if shots > 0 {
            neutralized as f64 / shots as f64
        } else {
            0.0
        };
        let eta_acc = if self.eta_rank_samples > 0 {
            self.eta_rank_hits as f64 / self.eta_rank_samples as f64
        } else {
            1.0
        };
        aegis_schema::ClosedLoopMetrics {
            auto_engage: self.auto_engage,
            completeness_at_decision_horizon: self.completeness_at_decision_horizon,
            decision_horizon_t: self.decision_horizon_t,
            eta_ranking_accuracy: eta_acc,
            eta_ranking_samples: self.eta_rank_samples,
            neutralize_fraction,
            time_to_neutralize_high_eta_s: self.time_to_neutralize_high_eta_s,
            jammer_activations: self.jammer_activations,
            kinetic_shots: self.kinetic_shots,
            jammer_on_rf_dark: self.jammer_on_rf_dark,
            asset_breaches: self.asset_breaches,
            neutralize_per_scarce_effector: neutralize_per,
            high_threat_neutralized: neutralized,
            auto_accepts: self.auto_accepts,
        }
    }
}

fn ready_slots(effectors: &[EffectorStatus]) -> (usize, usize) {
    let mut jammer = 0usize;
    let mut kinetic = 0usize;
    for e in effectors {
        if e.active || e.cooldown_remaining_s > 0.05 {
            continue;
        }
        match e.kind {
            EffectorKind::Jammer => jammer += 1,
            EffectorKind::Kinetic => kinetic += 1,
        }
    }
    (jammer, kinetic)
}

fn is_high_threat(r: &Recommendation, tracks: &[Track]) -> bool {
    if r.status != RecommendationStatus::Open || !is_mission_critical(r.action) {
        return false;
    }
    if r.threat_score >= HIGH_THREAT_SCORE {
        return true;
    }
    tracks
        .iter()
        .find(|tr| tr.id == r.track_id)
        .map(|tr| {
            tr.threat_score >= HIGH_THREAT_SCORE || tr.eta_s.map(|e| e < 45.0).unwrap_or(false)
        })
        .unwrap_or(false)
}

/// Select a batch of open MC recommendations (console Auto engage policy).
pub fn select_batch_package(
    open_high: &[Recommendation],
    effectors: &[EffectorStatus],
    tracks: &[Track],
) -> Vec<Recommendation> {
    let mut sorted: Vec<Recommendation> = open_high.to_vec();
    sorted.sort_by(|a, b| {
        b.threat_score
            .partial_cmp(&a.threat_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| match (a.eta_s, b.eta_s) {
                (Some(ea), Some(eb)) => ea.partial_cmp(&eb).unwrap_or(std::cmp::Ordering::Equal),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                _ => a.id.cmp(&b.id),
            })
    });
    if sorted.is_empty() {
        return Vec::new();
    }
    let rf_dark: std::collections::HashSet<Uuid> = tracks
        .iter()
        .filter(|tr| tr.rf_dark)
        .map(|tr| tr.id)
        .collect();
    let (mut jammer_slots, mut kinetic_slots) = ready_slots(effectors);
    let ready_total = jammer_slots + kinetic_slots;
    let slots = MAX_BATCH_ENGAGE.min(if ready_total > 0 {
        ready_total.max(1)
    } else {
        sorted.len().min(MAX_BATCH_ENGAGE)
    });
    let mut selected: Vec<Recommendation> = Vec::new();
    let match_ready = ready_total > 0;

    if match_ready && kinetic_slots > 0 {
        for rec in &sorted {
            if selected.len() >= slots || kinetic_slots == 0 {
                break;
            }
            if !rf_dark.contains(&rec.track_id) {
                continue;
            }
            if rec.action == RecommendedAction::EngageKinetic {
                selected.push(rec.clone());
                kinetic_slots -= 1;
            }
        }
    }
    if match_ready {
        for rec in &sorted {
            if selected.len() >= slots {
                break;
            }
            if selected.iter().any(|s| s.id == rec.id) {
                continue;
            }
            let dark = rf_dark.contains(&rec.track_id);
            if rec.action == RecommendedAction::RequestJammerAuthorization
                && jammer_slots > 0
                && !dark
            {
                selected.push(rec.clone());
                jammer_slots -= 1;
            } else if rec.action == RecommendedAction::EngageKinetic && kinetic_slots > 0 {
                selected.push(rec.clone());
                kinetic_slots -= 1;
            }
        }
    }
    for rec in &sorted {
        if selected.len() >= slots {
            break;
        }
        if selected.iter().any(|s| s.id == rec.id) {
            continue;
        }
        if rec.action == RecommendedAction::RequestJammerAuthorization
            && rf_dark.contains(&rec.track_id)
        {
            continue;
        }
        selected.push(rec.clone());
    }
    selected
}

fn apply_accept_effect(
    sim: &mut Simulation,
    recommend: &mut RecommendEngine,
    effect: AcceptEffect,
    track_pos: &[(Uuid, aegis_schema::Vec3)],
    accum: &mut ClosedLoopAccum,
) {
    match effect {
        AcceptEffect::CueEo { track_id } => {
            sim.cue_eo(track_id);
            recommend.annotate_effect_result("EO tasked");
        }
        AcceptEffect::AlertSector => {
            for z in sim.zones() {
                if matches!(z.kind, ZoneKind::KeepOut | ZoneKind::NoFly) {
                    recommend.mark_zone_alerted(z.id.clone());
                }
            }
            sim.apply_alert_sector();
            recommend.annotate_effect_result("keep-out / no-fly alerted");
        }
        AcceptEffect::ActivateJammer { track_id } => {
            let pos = track_pos
                .iter()
                .find(|(id, _)| *id == track_id)
                .map(|(_, p)| *p)
                .unwrap_or(aegis_schema::Vec3::zero());
            let note = sim.activate_jammer(track_id, pos);
            accum.jammer_activations += 1;
            recommend.annotate_effect_result(note);
        }
        AcceptEffect::EvacuatePad => {
            sim.apply_evacuate_pad();
            recommend.annotate_effect_result("pad evacuated");
        }
        AcceptEffect::HandOff { track_id: _ } => {
            recommend.annotate_effect_result("higher echelon notified");
        }
        AcceptEffect::EngageKinetic { track_id } => {
            let pos = track_pos
                .iter()
                .find(|(id, _)| *id == track_id)
                .map(|(_, p)| *p)
                .unwrap_or(aegis_schema::Vec3::zero());
            let note = sim.fire_kinetic(track_id, pos);
            accum.kinetic_shots += 1;
            recommend.annotate_effect_result(note);
        }
        AcceptEffect::None => {}
    }
}

/// Auto-accept high-threat MC package + one soft rec (Cue EO / alert) per tick.
pub fn auto_engage_tick(
    t: f64,
    sim: &mut Simulation,
    recommend: &mut RecommendEngine,
    tracks: &[Track],
    recs: &[Recommendation],
    accum: &mut ClosedLoopAccum,
) {
    let effectors = sim.effector_status();
    let open_high: Vec<Recommendation> = recs
        .iter()
        .filter(|r| is_high_threat(r, tracks))
        .cloned()
        .collect();
    let pkg = select_batch_package(&open_high, &effectors, tracks);
    let track_pos: Vec<_> = tracks.iter().map(|tr| (tr.id, tr.position)).collect();

    for rec in pkg {
        let disposed = recommend.dispose(
            t,
            rec.id,
            OperatorDisposition::Accepted,
            OperatorActor::Operator,
            Some(DispositionReasonCode::ApprovedBestOption),
        );
        if let Some((_ev, effect)) = disposed {
            accum.auto_accepts += 1;
            apply_accept_effect(sim, recommend, effect, &track_pos, accum);
        }
    }

    // Soft-first: accept one open soft Cue EO / Alert / Evac if present.
    if let Some(soft) = recs.iter().find(|r| {
        r.status == RecommendationStatus::Open
            && r.criticality == Criticality::Soft
            && matches!(
                r.action,
                RecommendedAction::CueEo
                    | RecommendedAction::AlertSector
                    | RecommendedAction::EvacuatePad
                    | RecommendedAction::MaintainWatch
            )
    }) {
        let disposed = recommend.dispose(
            t,
            soft.id,
            OperatorDisposition::Accepted,
            OperatorActor::Operator,
            Some(DispositionReasonCode::ApprovedBestOption),
        );
        if let Some((_ev, effect)) = disposed {
            accum.auto_accepts += 1;
            apply_accept_effect(sim, recommend, effect, &track_pos, accum);
        }
    }
}
