# Aegis

[![CI](https://github.com/dubare0898/Aegis/actions/workflows/ci.yml/badge.svg)](https://github.com/dubare0898/Aegis/actions/workflows/ci.yml)

**Aegis** is a simulation-first counter-UAS (C-UAS) **decision-support** platform: layered sensors feed deterministic fusion and doctrine-aware recommendations that **prioritize threats by impact and trajectory (time-to-arrival)** so scarce jammer/kinetic capacity is conserved for the right tracks—then the console authorizes soft or mission-critical effects (**Operator Y/N** by default, or optional **Auto engage** in sim).

## What this is / what this is not

| This is | This is not |
|---------|-------------|
| A **simulation** and engineering testbed for detect → track → recommend → authorize | A real-world weapons-control or fire-control system |
| Operator-in-the-loop decision support with explainable recommendations | A real autonomous lethal / weapons-release system |
| Optional **Auto engage** sim mode (no Y/N modal) for soak and stress runs | Authority to control physical effectors or real fire-control |
| Deterministic scenario generation, golden demos, batch/soak harnesses | A deployment-ready defense product |

**Jammer dwell and kinetic engagements in this repo are simulated only.** They exist to train and evaluate the decision loop, not to control physical effectors. **Auto engage** is a console sim convenience—still not real autonomous weapons.

## Key capabilities

- Multi-sensor simulation (radar, RF, EO/IR, ADS-B, acoustic) over a military FOB site pack
- Deterministic fusion + threat scoring + recommendation engine (impact/ETA ranking to conserve jammer & kinetic; fiber ≠ jammer-first; friendlies never mission-critical)
- Operator console: air picture with shape-aware legend glyphs, position lerp, ETA + rank rationale, batch **Engage high-level threats? Y/N**, HUD **Operator (Y/N)** / **Auto engage**, **Reset 42** + **Random seed**, enemies-downed log
- Seeded scenario **classes** (raid mixes, clutter, faults, friendly crossing) via harness / `aegis_scenario` (API loads the site pack; no interactive class picker over WS yet)
- Demo harness: golden replay, smoke suite, seed-batch metrics, long-run soak invariants
- `./scripts/check.sh` — workspace tests + smoke + `--assert-golden`

## Architecture

```
scenarios/ ──► aegis_scenario (generate) ──► ScenarioManifest
                      │
                      ▼
              aegis_sim (truth, sensors, effectors)
                      │
                      ▼
              aegis_fusion ──► aegis_recommend
                      │
          ┌───────────┴───────────┐
          ▼                       ▼
     aegis_api (WS)          demo_harness
          ▼
   apps/console (+ optional Tauri desktop)
```

## Repository structure

| Path | Role |
|------|------|
| `crates/aegis_schema` | Shared types (manifest, tracks, recommendations, metrics) |
| `crates/aegis_sim` | Truth motion, sensors, simulated effectors |
| `crates/aegis_fusion` | Deterministic track fusion |
| `crates/aegis_recommend` | Doctrine-aware recommendations + dispose |
| `crates/aegis_scenario` | Seeded class → manifest generation |
| `crates/aegis_api` | HTTP/WebSocket snapshot + commands |
| `apps/console` | React operator UI |
| `apps/desktop` | Optional Tauri shell |
| `scenarios/` | Site packs (golden: `military-base-swarm`) |
| `tools/demo_harness` | Golden, smoke, batch, soak |
| `DEMO.md` | 90-second evaluator script |

## Quickstart (native — primary)

**Requirements:** Rust stable, and for the UI: Node 20+.

```bash
git clone <your-fork-url>
cd aegis   # or whatever you named the clone

# Headless reliability bar
CARGO_TARGET_DIR="$PWD/target" cargo test --workspace
CARGO_TARGET_DIR="$PWD/target" cargo run -p demo_harness -- --suite smoke
CARGO_TARGET_DIR="$PWD/target" cargo run -p demo_harness -- --assert-golden

# Desktop (rebuilds console dist when src is newer; serves static dist)
./scripts/launch-desktop.sh
```

Two-terminal console (optional — preferred for UI iteration):

```bash
CARGO_TARGET_DIR="$PWD/target" cargo run -p aegis_api
cd apps/console && npm install && npm run dev
```

Desktop/`prepare-desktop-resources.sh` serve the built `apps/console/dist`. After console UI changes, either re-run `./scripts/launch-desktop.sh` (auto-rebuilds when `src` is newer than `dist`) or `cd apps/console && npm run build`. Use `npm run dev` for hot-reload while iterating on the operator UI.

Prefer `CARGO_TARGET_DIR="$PWD/target"` so builds stay inside the repo.

Quick validation bar (tests + smoke + golden):

```bash
./scripts/check.sh
```

## Optional Docker (headless reproducibility)

Containerize the **API + harness** (and a baked static console) for onboarding/CI-like reproducibility. This does **not** replace the native workflow and does **not** include the Tauri desktop app.

| In the image | Not in Docker |
|--------------|---------------|
| `aegis_api` on `0.0.0.0:8080` | Tauri / `./scripts/launch-desktop.sh` |
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

`aegis_scenario::generate(class, seed)` expands the military-base template into a resolved `ScenarioManifest`. **Same class + seed → identical JSON and sim outputs** when the run seed matches the generation seed.

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

- Durable KPI / run logging (JSONL + SQLite) for demo and soak runs
- Interactive scenario class picker over the API/WebSocket (harness already exercises classes)
- Stronger batch reporting / CI class sample
- Live-sensor adapter spike (read-only) behind a feature flag

## License / contributing / security

- License: [MIT](LICENSE)
- Contributing: [CONTRIBUTING.md](CONTRIBUTING.md)
- Security: [SECURITY.md](SECURITY.md)
- Changelog: [CHANGELOG.md](CHANGELOG.md)
