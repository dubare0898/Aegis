# Military Base — Layered Defense + Swarm

Primary 90-second demo scenario pack (`DEMO.md`).

## Narrative

A mixed decoy / RF-strike / fiber-optic UAV swarm ingresses FOB Sentinel. Layered radar, RF, EO/IR, acoustic, and ADS-B feeds produce imperfect detections. The console fuses tracks and recommends doctrine-aware actions. **Jammer and kinetic effects require operator confirmation**. RF-dark / fiber tracks must not collapse into jammer-first advice.

## Knobs (`scenario.json`)

- `swarm.count` / `decoy_fraction` / `fiber_fraction` / `start_range_m` / `cruise_speed_mps`
- sensor `pd` / `pfa` / `range_m`
- `effectors[]` jammer / kinetic ranges and Pk
- zone radii

Demo defaults favor a readable raid inside ~90s wall-clock at **4×** speed.

## Run

```bash
./scripts/launch-desktop.sh
# or
cargo run -p aegis_api -- --scenario military-base-swarm
cargo run -p demo_harness -- --seed 42 --ticks 600
```
