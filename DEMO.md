# Aegis — 90-second FOB demo

Golden reference scenario: layered sensing vs mixed RF + fiber swarm; operator confirms jammer/kinetic.

## Launch

```bash
./scripts/launch-desktop.sh
```

Sim stays **idle** until you press **Start**.

## Script (≈90 seconds wall-clock)

Use **4×** after Start so the raid reaches decision range inside the minute.

| t (wall) | You do | Evaluator should see |
|---------:|--------|----------------------|
| 0–10s | Point at legend: tracks, sensors, effectors, defended asset. Press **Start**, then **4×**. | Idle → running; detections → fused **track spheres**. |
| 10–35s | Open **Tracks**: call out `fiber-optic` / RF-dark vs normal swarm. | Same raid, two threat physics — not one jammer blob. |
| 35–55s | When **“Swarm detected. Engage defenses?”** appears: read proposed action. Click **Engage** *or* **Not now**. | Mission-critical effects only after operator yes. Soft actions stay on cards. |
| 55–75s | Soft path: Accept a **Cue EO** card (or HUD Cue EO on selected track). | EO joins provenance — recommend, not autopilot kill. |
| 75–90s | Optional: **Fail radar-N** briefly, then **Restore**. | Tracks coast; uncertainty — imperfect sensing. |

**Reset 42** to replay the same seed.

## Do say

- Detect → track → recommend → **operator authorize**.
- Fiber / RF-dark: soft-kill RF often ineffective; doctrine prefers cue / alert / kinetic — not jammer-first.
- Sim effectors train the decision loop; **not** autonomous weapons release.

## Do not say

- “The system engages automatically.”
- “Jammer handles the whole swarm.”
- Architecture digressions (packaging, multi-vertical, Gazebo).

## Verify offline

```bash
CARGO_TARGET_DIR="$PWD/target" cargo test --workspace
CARGO_TARGET_DIR="$PWD/target" cargo run -p demo_harness -- --suite smoke
```
