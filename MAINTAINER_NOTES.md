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

## Naming notes

- Cargo packages / binaries: `aegis_*` (e.g. `aegis_api`, `aegis_schema`)
- Scenario id kept as `military-base-swarm` (domain pack id, not product rename)
- Prose may still say **C-UAS** (counter-UAS domain term)
