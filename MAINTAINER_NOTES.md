# Maintainer notes (publish metadata)

## Suggested GitHub settings

| Field | Suggestion |
|-------|------------|
| Repository name | `Aegis` (remote: `dubare0898/Aegis`) |
| Description | `Simulation-first C-UAS decision support: detect → track → recommend → authorize (Operator Y/N or sim Auto engage)` |
| Topics | `cuas`, `simulation`, `rust`, `decision-support`, `sensor-fusion`, `deterministic-testing`, `operator-in-the-loop` |
| Website | (optional) demo video or portfolio page |
| License | MIT (see `LICENSE`) |

## After creating / updating the remote

1. Confirm the CI badge in `README.md` points at `dubare0898/Aegis` (already set).
2. Push `main` (or a PR branch) and confirm Actions runs green.
3. Local clone directory may be `aegis`; Cargo packages use `aegis_*`.
4. Run `./scripts/install-desktop-entry.sh` for a local menu entry (writes absolute paths under `~/.local/share/applications/` only).

## Naming notes

- Cargo packages / binaries: `aegis_*` (e.g. `aegis_api`, `aegis_schema`)
- Env: `AEGIS_PORT` for the API listen port
- Scenario id kept as `military-base-swarm` (domain pack id, not product rename)
- Prose may still say **C-UAS** (counter-UAS domain term)
- Do not document interactive WS class picker or KPI DB as shipped until they exist
