## Summary

## Test plan

- [ ] `cargo fmt --check`
- [ ] `CARGO_TARGET_DIR="$PWD/target" cargo test --workspace`
- [ ] `CARGO_TARGET_DIR="$PWD/target" cargo run -p demo_harness -- --suite smoke`
- [ ] Golden checked or intentionally updated (`--assert-golden` / `--write-golden`)

## Notes

- Risk / doctrine impact:
- Screenshots (console) if UI changed:
