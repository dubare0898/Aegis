# Aegis

[![CI](https://github.com/OWNER/aegis/actions/workflows/ci.yml/badge.svg)](https://github.com/OWNER/aegis/actions/workflows/ci.yml)

**Aegis** is a simulation-first counter-UAS (C-UAS) **decision-support** platform: layered sensors feed deterministic fusion and doctrine-aware recommendations that **prioritize threats by impact and trajectory (time-to-arrival)** so scarce jammer/kinetic capacity is conserved for the right tracks—then an operator authorizes soft or mission-critical effects in the loop.

> Replace `OWNER/aegis` in the badge URL after you create the GitHub repository (see [MAINTAINER_NOTES.md](MAINTAINER_NOTES.md)).

## What this is / what this is not

| This is | This is not |
|---------|-------------|
| A **simulation** and engineering testbed for detect → track → recommend → authorize | A real-world weapons-control or fire-control system |
| Operator-in-the-loop decision support with explainable recommendations | Autonomous lethal engagement |
| Deterministic scenario generation, golden demos, batch/soak harnesses | A deployment-ready defense product |

**Jammer dwell and kinetic engagements in this repo are simulated only.** They exist to train and evaluate the decision loop, not to control physical effectors.

## Key capabilities

- Multi-sensor simulation (radar, RF, EO/IR, ADS-B, acoustic) over a military FOB site pack
- Deterministic fusion + threat scoring + recommendation engine (impact/ETA ranking to conserve jammer & kinetic; fiber ≠ jammer-first; friendlies never mission-critical)
- Operator console: air picture, ETA + rank rationale on tracks/recs, engage confirmation, enemies-downed log
- Seeded scenario **classes** (raid mixes, clutter, faults, friendly crossing)
- Demo harness: golden replay, smoke suite, seed-batch metrics, long-run soak invariants

## Architecture

```
scenarios/ ──► cuas_scenario (generate) ──► ScenarioManifest
                      │
                      ▼
              cuas_sim (truth, sensors, effectors)
                      │
                      ▼
              cuas_fusion ──► cuas_recommend
                      │
          ┌───────────┴───────────┐
          ▼                       ▼
     cuas_api (WS)          demo_harness
          ▼
   apps/console (+ optional Tauri desktop)
```

## Repository structure

| Path | Role |
|------|------|
| `crates/cuas_schema` | Shared types (manifest, tracks, recommendations, metrics) |
| `crates/cuas_sim` | Truth motion, sensors, simulated effectors |
| `crates/cuas_fusion` | Deterministic track fusion |
| `crates/cuas_recommend` | Doctrine-aware recommendations + dispose |
| `crates/cuas_scenario` | Seeded class → manifest generation |
| `crates/cuas_api` | HTTP/WebSocket snapshot + commands |
| `apps/console` | React operator UI |
| `apps/desktop` | Optional Tauri shell |
| `scenarios/` | Site packs (golden: `military-base-swarm`) |
| `tools/demo_harness` | Golden, smoke, batch, soak |
| `DEMO.md` | 90-second evaluator script |

Rust package names remain `cuas_*` for stability; **Aegis** is the product name.

## Quickstart (native — primary)

**Requirements:** Rust stable, and for the UI: Node 20+.

```bash
git clone <your-fork-url>
cd aegis   # or whatever you named the clone

# Headless reliability bar
CARGO_TARGET_DIR="$PWD/target" cargo test --workspace
CARGO_TARGET_DIR="$PWD/target" cargo run -p demo_harness -- --suite smoke
CARGO_TARGET_DIR="$PWD/target" cargo run -p demo_harness -- --assert-golden

# Desktop (builds release API + console dist as needed)
./scripts/launch-desktop.sh
```

Two-terminal console (optional):

```bash
CARGO_TARGET_DIR="$PWD/target" cargo run -p cuas_api
cd apps/console && npm install && npm run dev
```

Prefer `CARGO_TARGET_DIR="$PWD/target"` so builds stay inside the repo.

## Optional Docker (headless reproducibility)

Containerize the **API + harness** (and a baked static console) for onboarding/CI-like reproducibility. This does **not** replace the native workflow and does **not** include the Tauri desktop app.

| In the image | Not in Docker |
|--------------|---------------|
| `cuas_api` on `0.0.0.0:8080` | Tauri / `./scripts/launch-desktop.sh` |
| `demo_harness` (smoke / batch / soak) | Hot-reload `npm run dev` |
| `scenarios/` + optional console `dist` | Day-to-day Rust edit/rebuild loop |

```bash
# Build once (includes console dist by default)
docker compose build

# API + static operator UI → http://localhost:8080
docker compose up api

# Harness one-shots (same image)
docker compose run --rm harness --suite smoke
docker compose run --rm harness --batch --class mixed_rf_dark_raid --seed-start 1 --seed-count 3 --ticks 400
docker compose run --rm harness --soak --class direct_swarm_raid --seed 3 --ticks 5000

# Smaller image without baking the console
docker compose build --build-arg BUILD_CONSOLE=0
```

**Tradeoffs:** first image build is slow (Rust release + optional Node). Console in the image is static (no HMR). Desktop stays native-only. Primary CI remains native `cargo test` + smoke; Docker is optional locally.

## Demo, batch, and soak

| Goal | Native | Docker |
|------|--------|--------|
| 90s script | [DEMO.md](DEMO.md) after `./scripts/launch-desktop.sh` | Browser at `http://localhost:8080` after `docker compose up api` |
| Smoke matrix | `cargo run -p demo_harness -- --suite smoke` | `docker compose run --rm harness --suite smoke` |
| Golden (seed 42) | `cargo run -p demo_harness -- --assert-golden` | Prefer native (goldens not required for smoke/batch/soak) |
| Batch classes | `cargo run -p demo_harness -- --batch --class all --seed-start 1 --seed-count 3 --ticks 400` | `docker compose run --rm harness --batch …` |
| Soak | `cargo run -p demo_harness -- --soak --class direct_swarm_raid --seed 3 --ticks 20000` | `docker compose run --rm harness --soak …` |

## Deterministic scenario generation

`cuas_scenario::generate(class, seed)` expands the military-base template into a resolved `ScenarioManifest`. **Same class + seed → identical JSON and sim outputs** when the run seed matches the generation seed.

Classes (v1):

- `direct_swarm_raid`
- `mixed_rf_dark_raid`
- `decoy_screen`
- `clutter_heavy_false_alarm_day`
- `friendly_crossing_with_hostile_ingress`
- `degraded_sensor_defense` (scheduled radar fault)

The hand-authored `scenarios/military-base-swarm` pack + seed **42** remains the golden FOB demo.

## Current limitations

- No live sensor adapters; all sensing is simulated
- No geospatial GIS stack; local ENU site model
- Desktop packaging is optional and Linux-oriented
- Not certified, accredited, or suitable for operational defense use

## Roadmap (near-term)

- Stronger batch reporting / CI class sample
- Optional crate/repo rename consistency (`aegis` on GitHub)
- Live-sensor adapter spike (read-only) behind a feature flag

## License / contributing / security

- License: [MIT](LICENSE)
- Contributing: [CONTRIBUTING.md](CONTRIBUTING.md)
- Security: [SECURITY.md](SECURITY.md)
- Changelog: [CHANGELOG.md](CHANGELOG.md)
