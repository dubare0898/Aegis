# Changelog

All notable changes to Aegis are documented here.

## [Unreleased]

### Added

- `aegis_harness` (renamed from `demo_harness`) with JSONL `RunMetrics` logging under `runs/`, closed-loop Auto-engage KPIs, and `--compare-baseline`
- Metric baseline gate (`tools/aegis_harness/baselines/metric_baseline.json`); CI runs golden + class sample
- Interactive scenario class picker over WS/API + console Class control
- Soft Accept consequences: AlertSector raises sensor attention; EvacuatePad clears friendlies
- Effector-state-aware recommend (ready/cooldown triage); fusion global greedy association + velocity gate
- Console track outcome line from effector `last_result` / operator notes

### Changed

- `DemoMetrics` → `RunMetrics`
- Fusion association under dense swarms; recommend capacity triage uses live effector status

### Notes

- Auto engage remains a **simulation** convenience for soak/stress—not real autonomous weapons or fire-control
- SQLite trending over JSONL is planned, not shipped

## [0.1.0] — 2026-07-26

### Added

- Simulation-first FOB scenario pack with mixed RF / fiber swarm
- Deterministic fusion, doctrine-aware recommendations, operator dispose flow
- Seeded scenario classes (`aegis_scenario`) and demo harness smoke / golden / batch / soak
- Enemies-downed defeat log (kinetic / jamming counters)
- Public-repo polish: MIT license, CI workflow, community templates

### Notes

- Product display name: **Aegis**; Rust crates/packages use `aegis_*`
