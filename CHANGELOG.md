# Changelog

All notable changes to Aegis are documented here.

## [Unreleased]

### Added

- Console engage modes: default **Operator (Y/N)** with batch **Engage high-level threats?** prompt; optional **Auto engage** (sim-only, no Y/N modal)
- **Random seed** control alongside **Reset 42**
- Shape-aware structure legend glyphs (ring / tri / diamond / hex / sphere) aligned with the air picture
- `./scripts/check.sh` — workspace tests + smoke + golden assert

### Changed

- Calmer swarm flight; fusion velocity EMA; recommend ETA hysteresis; AirScene position lerp for smoother tracks
- Earlier mission-critical thresholds, jammer divert behavior, and stronger simulated kinetins (golden baseline refreshed)

### Notes

- Auto engage remains a **simulation** convenience for soak/stress—not real autonomous weapons or fire-control
- Scenario class picker over WS and durable KPI/run logging are planned, not shipped

## [0.1.0] — 2026-07-26

### Added

- Simulation-first FOB scenario pack with mixed RF / fiber swarm
- Deterministic fusion, doctrine-aware recommendations, operator dispose flow
- Seeded scenario classes (`aegis_scenario`) and demo harness smoke / golden / batch / soak
- Enemies-downed defeat log (kinetic / jamming counters)
- Public-repo polish: MIT license, CI workflow, community templates

### Notes

- Product display name: **Aegis**; Rust crates/packages use `aegis_*`
