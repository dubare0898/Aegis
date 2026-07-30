# Aegis backlog (metric-tied)

North stars: completeness@horizon ↑ · ETA ranking accuracy ↑ · neutralize/scarce-effector ↑ · safety = 0.

## Done

- [x] Leak + jumpy flight + autonomous engage
- [x] Phase 0: JSONL `RunMetrics` logging + closed-loop Auto-engage scoring (`aegis_harness`)
- [x] Phase 0: CI golden + class sample + `--compare-baseline`
- [x] Cycle 1: Fusion association (global greedy, sensor gates, velocity consistency)
- [x] Cycle 2: Effector-state-aware recommend / capacity triage
- [x] Cycle 3: WS/API + console scenario class picker
- [x] Cycle 4: Soft-effect consequences + operator outcome feedback
- [x] Rename `demo_harness` → `aegis_harness`, `DemoMetrics` → `RunMetrics`

## Next (re-rank from JSONL trends)

1. [ ] SQLite reader/query over `runs/*.jsonl` for north-star trending
2. [ ] Cycle 5: Sensor friction (EO FOV/cue required, clutter→Pfa, jammer↔RF Pd)
3. [ ] Cycle 6: Enrich `military-base-swarm` + promote one stub template to a second runnable pack
4. [ ] Raise smoke floors after sustained north-star wins across seeds
5. [ ] Live-sensor adapter spike (read-only, feature-flagged) — only after KPIs can A/B it
