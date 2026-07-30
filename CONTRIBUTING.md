# Contributing to Aegis

Thanks for interest in improving Aegis. This is a simulation / decision-support engineering project; keep changes focused and testable.

## Development loop

```bash
CARGO_TARGET_DIR="$PWD/target" cargo fmt
CARGO_TARGET_DIR="$PWD/target" cargo test --workspace
CARGO_TARGET_DIR="$PWD/target" cargo run -p demo_harness -- --suite smoke
```

If you touch scenario timing or swarm motion, re-check golden:

```bash
CARGO_TARGET_DIR="$PWD/target" cargo run -p demo_harness -- --assert-golden
# only rewrite when intentionally changing the golden:
# cargo run -p demo_harness -- --write-golden --assert-golden
```

## Pull requests

- Prefer small, reviewable PRs with a clear “why”
- Do not commit `target/`, `node_modules/`, secrets, or local desktop path files
- Preserve determinism: same seed must replay
- Do not weaken doctrine/safety checks without explicit discussion (friendly engage, RF-dark jammer-first)

## Scope

Issues and PRs that fit well: harness metrics, scenario validity, fusion/recommend bugs, docs/CI, console clarity.

Out of scope unless agreed: real weapons interfaces, new verticals, heavy packaging work.
