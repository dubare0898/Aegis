## Summary

## Test plan

- [ ] `cargo fmt --check`
- [ ] `CARGO_TARGET_DIR="$PWD/target" cargo test --workspace`
- [ ] `CARGO_TARGET_DIR="$PWD/target" cargo run -p aegis_harness -- --suite smoke --compare-baseline`
- [ ] Golden checked or intentionally updated (`--assert-golden` / `--write-golden`)
- [ ] If KPI floors moved intentionally: baseline updated (`--write-baseline`)

## Notes

- Risk / doctrine impact:
- Screenshots (console) if UI changed:
