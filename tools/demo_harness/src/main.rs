mod scoring;

use anyhow::{bail, Context, Result};
use clap::Parser;
use cuas_fusion::FusionEngine;
use cuas_recommend::{is_mission_critical, RecommendEngine};
use cuas_scenario::generate;
use cuas_schema::{
    Affiliation, Criticality, DemoMetrics, DispositionReasonCode, GoldenSnapshot, OperatorActor,
    OperatorDisposition, RecommendedAction, ScenarioClass, ScenarioManifest,
};
use cuas_sim::{resolve_scenario_dir, Simulation};
use scoring::{score_tracks, truth_kinematics_finite, SmoothnessAccum, MAX_TRACK_COUNT};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "demo_harness")]
struct Args {
    #[arg(long, default_value = "military-base-swarm")]
    scenario: String,
    #[arg(long, default_value_t = 42)]
    seed: u64,
    #[arg(long, default_value_t = 600)]
    ticks: u64,
    #[arg(long, default_value_t = false)]
    json: bool,
    /// Run the smoke scenario matrix with pass/fail thresholds.
    #[arg(long)]
    suite: Option<String>,
    /// Assert golden snapshot at tick (default 120) matches committed file.
    #[arg(long, default_value_t = false)]
    assert_golden: bool,
    /// Rewrite golden snapshot file from this run.
    #[arg(long, default_value_t = false)]
    write_golden: bool,
    #[arg(long, default_value_t = 120)]
    golden_tick: u64,
    /// Batch generated scenario classes over a seed sweep.
    #[arg(long, default_value_t = false)]
    batch: bool,
    /// Scenario class name or `all` (batch/soak).
    #[arg(long)]
    class: Option<String>,
    #[arg(long, default_value_t = 1)]
    seed_start: u64,
    #[arg(long, default_value_t = 5)]
    seed_count: u64,
    /// Long-run soak with periodic invariant checks.
    #[arg(long, default_value_t = false)]
    soak: bool,
    /// Invariant sample period during soak (ticks).
    #[arg(long, default_value_t = 200)]
    soak_sample_every: u64,
}

#[derive(Clone)]
struct CaseSpec {
    name: &'static str,
    seed: u64,
    ticks: u64,
    mutate: fn(&mut ScenarioManifest),
    after_start: fn(&mut Simulation),
    /// min completeness, max false_track_rate, max rmse, max missed_rate
    thresholds: Thresholds,
}

#[derive(Clone, Copy)]
struct Thresholds {
    min_completeness: f64,
    max_false_rate: f64,
    max_rmse_m: f64,
    max_missed_rate: f64,
    require_recommend: bool,
}

fn noop_manifest(_: &mut ScenarioManifest) {}
fn noop_sim(_: &mut Simulation) {}

fn large_swarm(m: &mut ScenarioManifest) {
    m.swarm.count = 24;
}

fn high_decoy(m: &mut ScenarioManifest) {
    m.swarm.decoy_fraction = 0.6;
}

fn high_fiber(m: &mut ScenarioManifest) {
    m.swarm.fiber_fraction = 0.7;
    m.swarm.decoy_fraction = 0.1;
    // Bring the raid into acoustic envelopes within the smoke horizon.
    m.swarm.start_range_m = 2400.0;
    for s in &mut m.sensors {
        if s.kind == cuas_schema::SensorKind::Acoustic {
            s.pd = (s.pd + 0.08).min(0.92);
            s.range_m = s.range_m.max(1600.0);
        }
    }
}

fn low_pd(m: &mut ScenarioManifest) {
    for s in &mut m.sensors {
        if s.kind == cuas_schema::SensorKind::Radar {
            s.pd = 0.55;
        }
    }
}

fn fail_radar_north(sim: &mut Simulation) {
    sim.fail_sensor("radar-north");
}

fn smoke_cases() -> Vec<CaseSpec> {
    let base = Thresholds {
        min_completeness: 0.35,
        max_false_rate: 0.65,
        max_rmse_m: 220.0,
        max_missed_rate: 0.75,
        require_recommend: true,
    };
    vec![
        CaseSpec {
            name: "baseline",
            seed: 42,
            ticks: 400,
            mutate: noop_manifest,
            after_start: noop_sim,
            thresholds: base,
        },
        CaseSpec {
            name: "large_swarm",
            seed: 42,
            ticks: 400,
            mutate: large_swarm,
            after_start: noop_sim,
            thresholds: Thresholds {
                min_completeness: 0.25,
                max_false_rate: 0.75,
                ..base
            },
        },
        CaseSpec {
            name: "radar_north_failed",
            seed: 42,
            ticks: 400,
            mutate: noop_manifest,
            after_start: fail_radar_north,
            thresholds: Thresholds {
                min_completeness: 0.20,
                max_false_rate: 0.80,
                max_rmse_m: 280.0,
                ..base
            },
        },
        CaseSpec {
            name: "high_decoy",
            seed: 7,
            ticks: 400,
            mutate: high_decoy,
            after_start: noop_sim,
            thresholds: Thresholds {
                min_completeness: 0.25,
                max_false_rate: 0.75,
                ..base
            },
        },
        CaseSpec {
            name: "low_pd_radar",
            seed: 11,
            ticks: 500,
            mutate: low_pd,
            after_start: noop_sim,
            thresholds: Thresholds {
                min_completeness: 0.20,
                max_false_rate: 0.80,
                max_rmse_m: 260.0,
                ..base
            },
        },
        CaseSpec {
            name: "high_fiber_acoustic",
            seed: 19,
            ticks: 500,
            mutate: high_fiber,
            after_start: noop_sim,
            thresholds: Thresholds {
                min_completeness: 0.20,
                max_false_rate: 0.80,
                max_rmse_m: 280.0,
                max_missed_rate: 0.85,
                require_recommend: true,
            },
        },
    ]
}

struct RunResult {
    metrics: DemoMetrics,
}

fn run_case(
    scenario: &str,
    case_name: &str,
    seed: u64,
    ticks: u64,
    mutate: fn(&mut ScenarioManifest),
    after_start: fn(&mut Simulation),
    thresholds: Option<Thresholds>,
) -> Result<RunResult> {
    run_case_inner(
        scenario,
        case_name,
        seed,
        ticks,
        mutate,
        after_start,
        thresholds,
        None,
        false,
        200,
    )
}

fn run_case_inner(
    scenario: &str,
    case_name: &str,
    seed: u64,
    ticks: u64,
    mutate: fn(&mut ScenarioManifest),
    after_start: fn(&mut Simulation),
    thresholds: Option<Thresholds>,
    generated: Option<ScenarioManifest>,
    soak: bool,
    soak_sample_every: u64,
) -> Result<RunResult> {
    let mut sim = if let Some(m) = generated {
        Simulation::from_manifest(m, seed)
    } else {
        let dir = resolve_scenario_dir(scenario);
        let mut sim = Simulation::load(&dir, Some(seed))?;
        mutate(&mut sim.manifest);
        let manifest = sim.manifest.clone();
        Simulation::from_manifest(manifest, seed)
    };

    let mut fusion = FusionEngine::new(seed);
    let mut recommend = RecommendEngine::new();
    sim.start();
    after_start(&mut sim);

    let mut peak_tracks = 0usize;
    let mut peak_detections = 0usize;
    let mut multi_sensor_tracks = 0usize;
    let mut time_to_first_track: Option<f64> = None;
    let mut golden_at: Option<GoldenSnapshot> = None;
    let truth_hostiles = sim
        .truth_entities()
        .iter()
        .filter(|e| e.affiliation == cuas_schema::Affiliation::Hostile)
        .count();

    let mut last_tracks = Vec::new();
    let mut last_recs = Vec::new();
    let mut doctrine_ok = true;
    let mut kinematics_finite_ok = true;
    let mut fratricide_violations = 0usize;
    let mut safety_violations = 0usize;
    let mut recommendation_mix: BTreeMap<String, usize> = BTreeMap::new();
    let mut smoothness = SmoothnessAccum::default();
    let mut fault_applied: HashSet<(String, u8)> = HashSet::new();
    let class_label = sim.manifest.scenario_class.map(|c| c.as_str().to_string());

    for _ in 0..ticks {
        apply_fault_policy(&mut sim, &mut fault_applied);
        let detections = sim.step();
        let dt = sim.dt;
        let t = sim.t;
        let truth = sim.truth_entities();
        kinematics_finite_ok &= truth_kinematics_finite(&truth);
        smoothness.observe(&truth, dt);
        let mut tracks = fusion.process(t, dt, &detections);
        let track_pos: Vec<_> = tracks.iter().map(|tr| (tr.id, tr.position)).collect();
        sim.set_eo_truth_targets(&track_pos);
        let recs = recommend.evaluate(t, &mut tracks, sim.zones());
        let (doc_ok, frat, safe) = doctrine_stats(&tracks, &recs);
        doctrine_ok &= doc_ok;
        fratricide_violations += frat;
        safety_violations += safe;
        for r in &recs {
            *recommendation_mix.entry(action_key(r.action)).or_default() += 1;
        }

        let hostile_tracks = tracks
            .iter()
            .filter(|tr| tr.affiliation != cuas_schema::Affiliation::Friendly)
            .count();
        if time_to_first_track.is_none() && hostile_tracks > 0 {
            time_to_first_track = Some(t);
        }

        peak_tracks = peak_tracks.max(tracks.len());
        peak_detections = peak_detections.max(detections.len());
        multi_sensor_tracks = multi_sensor_tracks.max(
            tracks
                .iter()
                .filter(|tr| tr.sensor_provenance.len() >= 2)
                .count(),
        );

        if soak && sim.tick % soak_sample_every == 0 {
            if tracks.len() > MAX_TRACK_COUNT {
                bail!("soak track explosion at t={t:.1}s tracks={}", tracks.len());
            }
            if !truth_kinematics_finite(&truth) {
                bail!("soak NaN/Inf kinematics at t={t:.1}s");
            }
            if !doc_ok {
                bail!("soak doctrine violation at t={t:.1}s");
            }
        }

        if sim.tick == 120 {
            golden_at = Some(sim.golden_snapshot(&tracks));
        }
        last_tracks = tracks;
        last_recs = recs;
    }

    // Soft accept must produce an operator audit event.
    let soft_audit_ok = soft_accept_audits(&mut recommend, sim.t, &last_recs);
    // Impact/ETA ranking: top open inbound should not be dramatically looser than the set.
    let rank_ok = rank_eta_invariant(&last_recs);

    if golden_at.is_none() {
        golden_at = Some(sim.golden_snapshot(&last_tracks));
    }

    let score = score_tracks(&sim.truth_entities(), &last_tracks, 250.0);

    // Determinism: second run same seed/mutation
    let mut sim_b = Simulation::from_manifest(sim.manifest.clone(), seed);
    let mut fusion_b = FusionEngine::new(seed);
    let mut recommend_b = RecommendEngine::new();
    sim_b.start();
    after_start(&mut sim_b);
    let mut tracks_b = Vec::new();
    for _ in 0..ticks {
        let detections = sim_b.step();
        tracks_b = fusion_b.process(sim_b.t, sim_b.dt, &detections);
        let track_pos: Vec<_> = tracks_b.iter().map(|tr| (tr.id, tr.position)).collect();
        sim_b.set_eo_truth_targets(&track_pos);
        let _ = recommend_b.evaluate(sim_b.t, &mut tracks_b, sim_b.zones());
    }
    let golden_b = sim_b.golden_snapshot(&tracks_b);
    let golden = golden_at.unwrap();
    // Compare mid-run golden if both have tick 120; else compare finals
    let det_ok = if ticks >= 120 {
        let mut sa = Simulation::from_manifest(sim.manifest.clone(), seed);
        let mut fa = FusionEngine::new(seed);
        sa.start();
        after_start(&mut sa);
        let mut ta = Vec::new();
        for _ in 0..120 {
            let d = sa.step();
            ta = fa.process(sa.t, sa.dt, &d);
        }
        let ga = sa.golden_snapshot(&ta);
        let mut sb = Simulation::from_manifest(sim.manifest.clone(), seed);
        let mut fb = FusionEngine::new(seed);
        sb.start();
        after_start(&mut sb);
        let mut tb = Vec::new();
        for _ in 0..120 {
            let d = sb.step();
            tb = fb.process(sb.t, sb.dt, &d);
        }
        let gb = sb.golden_snapshot(&tb);
        ga == gb
    } else {
        golden == golden_b
    };

    let max_heading_delta_rad = smoothness.max_heading_delta_rad();
    let p95_accel_mps2 = smoothness.p95_accel_mps2();
    let smoothness_violations = smoothness.violation_count();
    let kinematics_ok = smoothness.kinematics_ok() && kinematics_finite_ok;
    let safety_ok = fratricide_violations == 0 && safety_violations == 0 && doctrine_ok;

    let mut passed = det_ok
        && soft_audit_ok
        && rank_ok
        && kinematics_ok
        && safety_ok
        && peak_tracks <= MAX_TRACK_COUNT;
    if let Some(th) = thresholds {
        passed &= score.track_completeness >= th.min_completeness;
        passed &= score.false_track_rate <= th.max_false_rate;
        passed &= score.position_rmse_m <= th.max_rmse_m;
        passed &= score.missed_truth_rate <= th.max_missed_rate;
        if th.require_recommend {
            passed &= recommend.first_recommend_t().is_some();
        }
    }

    if case_name == "high_fiber_acoustic" {
        passed &= fiber_acoustic_ok(&sim.truth_entities(), &last_tracks);
    }

    let metrics = DemoMetrics {
        case: case_name.to_string(),
        seed,
        ticks,
        final_t: sim.t,
        truth_hostiles,
        peak_tracks,
        final_tracks: last_tracks.len(),
        matched_tracks: score.matched,
        false_tracks: score.false_tracks,
        missed_truth: score.missed_truth,
        track_purity: score.track_purity,
        track_completeness: score.track_completeness,
        false_track_rate: score.false_track_rate,
        missed_truth_rate: score.missed_truth_rate,
        position_rmse_m: score.position_rmse_m,
        time_to_first_track_s: time_to_first_track,
        time_to_first_recommend_s: recommend.first_recommend_t(),
        recommendations_issued: recommend.issued_count(),
        multi_sensor_tracks,
        deterministic_ok: det_ok,
        passed,
        max_heading_delta_rad,
        p95_accel_mps2,
        smoothness_violations,
        kinematics_ok,
        fratricide_violations,
        safety_violations,
        recommendation_mix,
        peak_detections,
        scenario_class: class_label,
    };

    let _ = golden;
    Ok(RunResult { metrics })
}

fn apply_fault_policy(sim: &mut Simulation, applied: &mut HashSet<(String, u8)>) {
    let events = sim.manifest.fault_policy.events.clone();
    let t = sim.t;
    for ev in events {
        if t + 1e-9 >= ev.fail_at_s && applied.insert((ev.sensor_id.clone(), 0)) {
            sim.fail_sensor(&ev.sensor_id);
        }
        if let Some(restore) = ev.restore_at_s {
            if t + 1e-9 >= restore && applied.insert((ev.sensor_id.clone(), 1)) {
                sim.restore_sensor(&ev.sensor_id);
            }
        }
    }
}

fn action_key(a: RecommendedAction) -> String {
    match a {
        RecommendedAction::CueEo => "cue_eo",
        RecommendedAction::AlertSector => "alert_sector",
        RecommendedAction::RequestJammerAuthorization => "request_jammer_authorization",
        RecommendedAction::EvacuatePad => "evacuate_pad",
        RecommendedAction::HandOffHigherEchelon => "hand_off_higher_echelon",
        RecommendedAction::EngageKinetic => "engage_kinetic",
        RecommendedAction::MaintainWatch => "maintain_watch",
    }
    .into()
}

/// Fiber hostiles should be tracked via radar/acoustic with little RF provenance.
fn fiber_acoustic_ok(truth: &[cuas_schema::TruthEntity], tracks: &[cuas_schema::Track]) -> bool {
    use cuas_schema::{Affiliation, SensorKind};
    let fiber: Vec<_> = truth
        .iter()
        .filter(|e| e.rf_dark && e.affiliation == Affiliation::Hostile)
        .collect();
    if fiber.is_empty() {
        return true;
    }
    let mut matched_ok = 0usize;
    let mut rf_heavy = 0usize;
    for f in &fiber {
        let Some(tr) = tracks
            .iter()
            .filter(|tr| tr.affiliation != Affiliation::Friendly)
            .min_by(|a, b| {
                a.position
                    .distance(&f.position)
                    .partial_cmp(&b.position.distance(&f.position))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .filter(|tr| tr.position.distance(&f.position) <= 320.0)
        else {
            continue;
        };
        let has_radar = tr.sensor_provenance.contains(&SensorKind::Radar);
        let has_acoustic = tr.sensor_provenance.contains(&SensorKind::Acoustic);
        let has_rf = tr.sensor_provenance.contains(&SensorKind::Rf);
        if has_radar && has_acoustic {
            matched_ok += 1;
        }
        if has_rf && !has_acoustic {
            rf_heavy += 1;
        }
    }
    matched_ok >= 1 && rf_heavy <= matched_ok
}

/// Among open inbound recommendations, P1 should be among the tighter ETAs (impact can reorder near-ties).
fn rank_eta_invariant(recs: &[cuas_schema::Recommendation]) -> bool {
    let mut inbound: Vec<(u32, f64)> = recs
        .iter()
        .filter(|r| r.status == cuas_schema::RecommendationStatus::Open)
        .filter_map(|r| r.eta_s.map(|eta| (r.priority, eta)))
        .collect();
    if inbound.len() < 2 {
        return true;
    }
    inbound.sort_by_key(|(p, _)| *p);
    let top_eta = inbound[0].1;
    let mut etas: Vec<f64> = inbound.iter().map(|(_, e)| *e).collect();
    etas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let tightest = etas[0];
    let median = etas[etas.len() / 2];
    // P1 may lose a near-tie to zone/swarm weights, but must not be the clear straggler.
    top_eta <= median * 1.75 + 20.0 && top_eta <= tightest * 3.0 + 25.0
}

/// Returns (ok, fratricide_count, other_safety_count).
fn doctrine_stats(
    tracks: &[cuas_schema::Track],
    recs: &[cuas_schema::Recommendation],
) -> (bool, usize, usize) {
    let mut frat = 0usize;
    let mut safe = 0usize;
    for r in recs {
        if let Some(tr) = tracks.iter().find(|t| t.id == r.track_id) {
            if tr.affiliation == Affiliation::Friendly && is_mission_critical(r.action) {
                frat += 1;
            }
            if (tr.rf_dark || tr.class_hypothesis == cuas_schema::TrackClass::FiberOpticUas)
                && r.action == RecommendedAction::RequestJammerAuthorization
            {
                safe += 1;
            }
        }
        if is_mission_critical(r.action)
            && (!r.requires_confirmation || r.criticality != Criticality::MissionCritical)
        {
            safe += 1;
        }
    }
    (frat == 0 && safe == 0, frat, safe)
}

fn soft_accept_audits(
    recommend: &mut RecommendEngine,
    t: f64,
    recs: &[cuas_schema::Recommendation],
) -> bool {
    let soft = recs.iter().find(|r| {
        r.status == cuas_schema::RecommendationStatus::Open
            && r.criticality == Criticality::Soft
            && !r.requires_confirmation
            && matches!(
                r.action,
                RecommendedAction::CueEo
                    | RecommendedAction::AlertSector
                    | RecommendedAction::MaintainWatch
                    | RecommendedAction::EvacuatePad
                    | RecommendedAction::HandOffHigherEchelon
            )
    });
    let Some(soft) = soft else {
        // No soft open rec this tick — don't fail the suite on that alone.
        return true;
    };
    let before = recommend.operator_events().len();
    let disposed = recommend.dispose(
        t,
        soft.id,
        OperatorDisposition::Accepted,
        OperatorActor::Operator,
        Some(DispositionReasonCode::ApprovedBestOption),
    );
    disposed.is_some() && recommend.operator_events().len() > before
}

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("goldens/baseline_seed42_tick120.json")
}

fn print_metrics(m: &DemoMetrics) {
    println!("case:                 {}", m.case);
    println!("  seed:               {}", m.seed);
    println!("  ticks:              {}", m.ticks);
    println!("  final_t:            {:.1}s", m.final_t);
    println!("  truth_hostiles:     {}", m.truth_hostiles);
    println!("  peak_tracks:        {}", m.peak_tracks);
    println!("  final_tracks:       {}", m.final_tracks);
    println!("  matched:            {}", m.matched_tracks);
    println!("  false_tracks:       {}", m.false_tracks);
    println!("  missed_truth:       {}", m.missed_truth);
    println!("  purity:             {:.2}", m.track_purity);
    println!("  completeness:       {:.2}", m.track_completeness);
    println!("  false_rate:         {:.2}", m.false_track_rate);
    println!("  missed_rate:        {:.2}", m.missed_truth_rate);
    println!("  rmse_m:             {:.1}", m.position_rmse_m);
    println!(
        "  t_first_track:      {}",
        m.time_to_first_track_s
            .map(|t| format!("{t:.2}s"))
            .unwrap_or_else(|| "n/a".into())
    );
    println!(
        "  t_first_recommend:  {}",
        m.time_to_first_recommend_s
            .map(|t| format!("{t:.2}s"))
            .unwrap_or_else(|| "n/a".into())
    );
    println!("  recommendations:    {}", m.recommendations_issued);
    println!("  multi_sensor_peak:  {}", m.multi_sensor_tracks);
    println!("  max_heading_d_rad:  {:.3}", m.max_heading_delta_rad);
    println!("  p95_accel_mps2:     {:.1}", m.p95_accel_mps2);
    println!("  smooth_violations:  {}", m.smoothness_violations);
    println!(
        "  kinematics_ok:      {}",
        if m.kinematics_ok { "PASS" } else { "FAIL" }
    );
    println!("  fratricide:         {}", m.fratricide_violations);
    println!("  safety_violations:  {}", m.safety_violations);
    println!("  peak_detections:    {}", m.peak_detections);
    if let Some(c) = &m.scenario_class {
        println!("  scenario_class:     {c}");
    }
    if !m.recommendation_mix.is_empty() {
        let mix: Vec<_> = m
            .recommendation_mix
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect();
        println!("  rec_mix:            {}", mix.join(" "));
    }
    println!(
        "  deterministic:      {}",
        if m.deterministic_ok { "PASS" } else { "FAIL" }
    );
    println!(
        "  thresholds:         {}",
        if m.passed { "PASS" } else { "FAIL" }
    );
}

fn handle_golden(args: &Args, snapshot: &GoldenSnapshot) -> Result<()> {
    let path = golden_path();
    // Canonical JSON string compare — avoids f64 round-trip PartialEq false negatives.
    let current = serde_json::to_string_pretty(snapshot)?;
    if args.write_golden {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &current)?;
        println!("wrote golden {}", path.display());
    }
    if args.assert_golden {
        let expected = fs::read_to_string(&path).with_context(|| {
            format!("missing golden {}; run with --write-golden", path.display())
        })?;
        if expected != current {
            bail!("golden snapshot mismatch at {}", path.display());
        }
        println!("golden OK {}", path.display());
    }
    Ok(())
}

fn parse_classes(spec: &str) -> Result<Vec<ScenarioClass>> {
    if spec == "all" {
        return Ok(ScenarioClass::all().to_vec());
    }
    ScenarioClass::parse(spec)
        .map(|c| vec![c])
        .ok_or_else(|| anyhow::anyhow!("unknown class '{spec}'"))
}

fn run_batch(args: &Args) -> Result<()> {
    let class_spec = args
        .class
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--batch requires --class <name>|all"))?;
    let classes = parse_classes(class_spec)?;
    println!(
        "CUAS batch: classes={} seeds={}..{}",
        classes.len(),
        args.seed_start,
        args.seed_start + args.seed_count.saturating_sub(1)
    );
    let mut all_ok = true;
    let mut results = Vec::new();
    for class in classes {
        for i in 0..args.seed_count {
            let seed = args.seed_start + i;
            let manifest = generate(class, seed).context("generate scenario")?;
            let case = format!("{}-{}", class.as_str(), seed);
            let result = run_case_inner(
                &args.scenario,
                &case,
                seed,
                args.ticks,
                noop_manifest,
                noop_sim,
                None,
                Some(manifest),
                false,
                args.soak_sample_every,
            )?;
            print_metrics(&result.metrics);
            println!();
            all_ok &= result.metrics.passed;
            results.push(result.metrics);
        }
    }
    if args.json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    }
    if !all_ok {
        bail!("batch failed");
    }
    println!("batch: PASS ({} runs)", results.len());
    Ok(())
}

fn run_soak(args: &Args) -> Result<()> {
    let class = args
        .class
        .as_deref()
        .and_then(ScenarioClass::parse)
        .unwrap_or(ScenarioClass::DirectSwarmRaid);
    let ticks = if args.ticks < 1000 {
        20_000
    } else {
        args.ticks
    };
    println!(
        "CUAS soak: class={} seed={} ticks={}",
        class.as_str(),
        args.seed,
        ticks
    );
    let manifest = generate(class, args.seed)?;
    let result = run_case_inner(
        &args.scenario,
        &format!("soak-{}", class.as_str()),
        args.seed,
        ticks,
        noop_manifest,
        noop_sim,
        None,
        Some(manifest),
        true,
        args.soak_sample_every,
    )?;
    print_metrics(&result.metrics);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&result.metrics)?);
    }
    if !result.metrics.passed {
        bail!("soak failed");
    }
    println!("soak: PASS");
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.batch {
        return run_batch(&args);
    }
    if args.soak {
        return run_soak(&args);
    }

    if let Some(suite) = &args.suite {
        if suite != "smoke" {
            bail!("unknown suite '{suite}' (supported: smoke)");
        }
        println!("CUAS smoke suite");
        let mut all_ok = true;
        let mut results = Vec::new();
        for case in smoke_cases() {
            let result = run_case(
                &args.scenario,
                case.name,
                case.seed,
                case.ticks,
                case.mutate,
                case.after_start,
                Some(case.thresholds),
            )?;
            print_metrics(&result.metrics);
            println!();
            all_ok &= result.metrics.passed;
            results.push(result.metrics);
        }
        if args.json {
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        if !all_ok {
            bail!("smoke suite failed");
        }
        println!("smoke suite: PASS");
        return Ok(());
    }

    let result = run_case(
        &args.scenario,
        "single",
        args.seed,
        args.ticks,
        noop_manifest,
        noop_sim,
        None,
    )?;

    // Build golden at requested tick for assert (hand military-base-swarm only).
    let dir = resolve_scenario_dir(&args.scenario);
    let mut sim = Simulation::load(&dir, Some(args.seed))?;
    let mut fusion = FusionEngine::new(args.seed);
    sim.start();
    let mut tracks = Vec::new();
    for _ in 0..args.golden_tick {
        let d = sim.step();
        tracks = fusion.process(sim.t, sim.dt, &d);
    }
    let golden = sim.golden_snapshot(&tracks);
    handle_golden(&args, &golden)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result.metrics)?);
    } else {
        println!("CUAS demo harness");
        print_metrics(&result.metrics);
    }

    if !result.metrics.deterministic_ok {
        bail!("deterministic replay check failed");
    }
    Ok(())
}

#[allow(dead_code)]
fn ensure_exists(p: &Path) -> bool {
    p.exists()
}
