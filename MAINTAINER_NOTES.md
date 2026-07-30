# Maintainer notes (publish metadata)

## Suggested GitHub settings

| Field | Suggestion |
|-------|------------|
| Repository name | `aegis` or `aegis-cuas` |
| Description | `Simulation-first C-UAS decision support: detect → track → recommend → operator authorize` |
| Topics | `cuas`, `simulation`, `rust`, `decision-support`, `sensor-fusion`, `deterministic-testing`, `operator-in-the-loop` |
| Website | (optional) demo video or portfolio page |
| License | MIT (see `LICENSE`) |

## After creating the remote

1. Replace `OWNER/aegis` in `README.md` CI badge with your org/user and repo name.
2. Push `main` and confirm Actions runs green.
3. Optional: rename local folder to match the remote (`aegis`).
4. Run `./scripts/install-desktop-entry.sh` for a local menu entry (writes absolute paths under `~/.local/share/applications/` only).

## Intentionally not renamed this pass

- Cargo package names: `cuas_schema`, `cuas_sim`, …
- Scenario id: `military-base-swarm`
- Binary / launcher script names under `scripts/`

Deeper rename is optional follow-up once the public repo URL is stable.
