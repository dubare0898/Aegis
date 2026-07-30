//! JSONL run logging and metric baseline comparison.

use aegis_schema::{HarnessRunRecord, RunMetrics, HARNESS_RUN_SCHEMA_VERSION};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn default_log_dir() -> PathBuf {
    PathBuf::from("runs")
}

pub fn git_sha() -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
}

pub fn append_run(log_dir: &Path, mode: &str, metrics: &RunMetrics) -> Result<PathBuf> {
    fs::create_dir_all(log_dir).with_context(|| format!("create log dir {}", log_dir.display()))?;
    let day = chrono_like_date();
    let path = log_dir.join(format!("harness-{day}.jsonl"));
    let record = HarnessRunRecord {
        schema_version: HARNESS_RUN_SCHEMA_VERSION,
        git_sha: git_sha(),
        mode: mode.to_string(),
        case: metrics.case.clone(),
        seed: metrics.seed,
        ticks: metrics.ticks,
        metrics: metrics.clone(),
    };
    let line = serde_json::to_string(&record)?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    writeln!(f, "{line}")?;
    Ok(path)
}

fn chrono_like_date() -> String {
    // Prefer UTC date via `date` for zero deps; fall back to local process time.
    if let Ok(out) = Command::new("date").args(["-u", "+%Y-%m-%d"]).output() {
        if out.status.success() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                let t = s.trim();
                if !t.is_empty() {
                    return t.to_string();
                }
            }
        }
    }
    "local".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricBaseline {
    pub schema_version: u32,
    /// Allowed absolute regression on completeness-like scores.
    pub eps_completeness: f64,
    pub eps_eta_ranking: f64,
    pub eps_neutralize_per: f64,
    /// Hard floors (must not fall below even within eps).
    pub floors: BaselineFloors,
    /// Per-smoke-case minimum completeness (from last known good).
    pub smoke_min_completeness: std::collections::BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineFloors {
    pub min_completeness_at_decision_horizon: f64,
    pub min_eta_ranking_accuracy: f64,
    pub max_jammer_on_rf_dark: usize,
    pub max_safety_violations: usize,
    pub min_neutralize_fraction_auto: f64,
}

impl Default for MetricBaseline {
    fn default() -> Self {
        Self {
            schema_version: 1,
            eps_completeness: 0.05,
            eps_eta_ranking: 0.10,
            eps_neutralize_per: 0.20,
            floors: BaselineFloors {
                min_completeness_at_decision_horizon: 0.12,
                min_eta_ranking_accuracy: 0.40,
                max_jammer_on_rf_dark: 0,
                max_safety_violations: 0,
                min_neutralize_fraction_auto: 0.05,
            },
            smoke_min_completeness: [
                ("baseline", 0.35),
                ("large_swarm", 0.25),
                ("radar_north_failed", 0.20),
                ("high_decoy", 0.25),
                ("low_pd_radar", 0.20),
                ("high_fiber_acoustic", 0.20),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        }
    }
}

pub fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("baselines/metric_baseline.json")
}

pub fn load_baseline(path: &Path) -> Result<MetricBaseline> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("missing baseline {}; write defaults first", path.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn write_baseline(path: &Path, baseline: &MetricBaseline) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(baseline)? + "\n")?;
    Ok(())
}

pub fn compare_run(baseline: &MetricBaseline, m: &RunMetrics) -> Result<()> {
    let mut errs = Vec::new();
    if m.safety_violations > baseline.floors.max_safety_violations || m.fratricide_violations > 0 {
        errs.push(format!(
            "{}: safety/fratricide violations (safety={}, frat={})",
            m.case, m.safety_violations, m.fratricide_violations
        ));
    }
    let cl = &m.closed_loop;
    if let Some(h) = cl.completeness_at_decision_horizon {
        let floor = baseline.floors.min_completeness_at_decision_horizon;
        if h + 1e-9 < floor {
            errs.push(format!(
                "{}: completeness@horizon {h:.3} < floor {floor:.3}",
                m.case
            ));
        }
        if let Some(smoke_floor) = baseline.smoke_min_completeness.get(&m.case) {
            if m.track_completeness + baseline.eps_completeness + 1e-9 < *smoke_floor {
                errs.push(format!(
                    "{}: completeness {:.3} regressed below smoke floor {:.3} (eps={})",
                    m.case, m.track_completeness, smoke_floor, baseline.eps_completeness
                ));
            }
        }
    }
    if cl.eta_ranking_samples > 0
        && cl.eta_ranking_accuracy + 1e-9
            < baseline.floors.min_eta_ranking_accuracy - baseline.eps_eta_ranking
    {
        errs.push(format!(
            "{}: eta_ranking_accuracy {:.3} below floor",
            m.case, cl.eta_ranking_accuracy
        ));
    }
    if cl.jammer_on_rf_dark > baseline.floors.max_jammer_on_rf_dark {
        errs.push(format!(
            "{}: jammer_on_rf_dark={} exceeds max {}",
            m.case, cl.jammer_on_rf_dark, baseline.floors.max_jammer_on_rf_dark
        ));
    }
    // Only gate neutralize when the closed loop actually tasked scarce effectors.
    let shots = cl.jammer_activations + cl.kinetic_shots;
    if cl.auto_engage
        && shots > 0
        && cl.neutralize_fraction + 1e-9 < baseline.floors.min_neutralize_fraction_auto
    {
        errs.push(format!(
            "{}: neutralize_fraction {:.3} < auto floor {:.3} (shots={shots})",
            m.case, cl.neutralize_fraction, baseline.floors.min_neutralize_fraction_auto
        ));
    }
    if !errs.is_empty() {
        bail!("baseline compare failed:\n  - {}", errs.join("\n  - "));
    }
    Ok(())
}

pub fn compare_suite(baseline: &MetricBaseline, results: &[RunMetrics]) -> Result<()> {
    let mut all_errs = Vec::new();
    for m in results {
        if let Err(e) = compare_run(baseline, m) {
            all_errs.push(e.to_string());
        }
    }
    // Aggregate north-star means across auto-engage runs.
    let auto: Vec<_> = results
        .iter()
        .filter(|m| m.closed_loop.auto_engage)
        .collect();
    if !auto.is_empty() {
        let mean_horizon: f64 = auto
            .iter()
            .filter_map(|m| m.closed_loop.completeness_at_decision_horizon)
            .sum::<f64>()
            / auto
                .iter()
                .filter(|m| m.closed_loop.completeness_at_decision_horizon.is_some())
                .count()
                .max(1) as f64;
        if mean_horizon + 1e-9 < baseline.floors.min_completeness_at_decision_horizon {
            all_errs.push(format!(
                "suite mean completeness@horizon {mean_horizon:.3} below floor"
            ));
        }
    }
    if !all_errs.is_empty() {
        bail!("{}", all_errs.join("\n"));
    }
    println!(
        "baseline compare: PASS ({} runs vs {})",
        results.len(),
        "metric_baseline"
    );
    Ok(())
}
