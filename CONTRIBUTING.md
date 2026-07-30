# Contributing to Aegis

Thanks for interest in improving Aegis. This is a simulation / decision-support engineering project; keep changes focused and testable.

## Development loop

Prefer the one-shot reliability bar:

```bash
./scripts/check.sh
```

That runs `cargo test --workspace`, `aegis_harness --suite smoke --compare-baseline`, and `--assert-golden`.

For iterative work:

```bash
CARGO_TARGET_DIR="$PWD/target" cargo fmt
CARGO_TARGET_DIR="$PWD/target" cargo test --workspace
CARGO_TARGET_DIR="$PWD/target" cargo run -p aegis_harness -- --suite smoke --compare-baseline
```

Smoke/batch/soak append `RunMetrics` JSONL under `runs/` (gitignored) unless you pass `--no-log`.

If you touch scenario timing, swarm motion, fusion, recommend, or effectors, re-check golden (included in `./scripts/check.sh`):

```bash
CARGO_TARGET_DIR="$PWD/target" cargo run -p aegis_harness -- --assert-golden --no-auto-engage --no-log
# only rewrite when intentionally changing the golden:
# cargo run -p aegis_harness -- --write-golden --assert-golden --no-auto-engage --no-log
```

Intentional KPI floor changes: rewrite with `--write-baseline` and commit `tools/aegis_harness/baselines/metric_baseline.json`.

After console UI changes, rebuild `apps/console/dist` (`npm run build` or relaunch `./scripts/launch-desktop.sh`) before serving desktop/static.

## Pull requests

- Prefer small, reviewable PRs with a clear “why”
- Do not commit `target/`, `node_modules/`, secrets, or local desktop path files
- Preserve determinism: same seed must replay
- Do not weaken doctrine/safety checks without explicit discussion (friendly engage, RF-dark jammer-first)
- Keep public docs honest: simulated effectors; Auto engage is sim-only, not real autonomous weapons

## Scope

Issues and PRs that fit well: north-star KPI improvements (completeness@horizon, ETA ranking, neutralize/scarce-effector, safety=0), scenario validity, fusion/recommend bugs, docs/CI, console OITL clarity, SQLite trending over JSONL.

Out of scope unless agreed: real weapons interfaces, new verticals, heavy packaging work.
