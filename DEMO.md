# Aegis — 90-second FOB demo

Golden reference scenario: layered sensing vs mixed RF + fiber swarm; default path is operator Y/N for jammer/kinetic. Optional **Auto engage** is a sim mode (no Y/N)—still simulated effectors, not real autonomous weapons.

## Launch

```bash
./scripts/launch-desktop.sh
```

Sim stays **idle** until you press **Start**. After console UI changes, relaunch (or `cd apps/console && npm run build`) so `dist` is not stale.

## Script (≈90 seconds wall-clock)

Use **4×** after Start so the raid reaches decision range inside the minute. Leave HUD on **Operator (Y/N)** unless you explicitly want Auto.

| t (wall) | You do | Evaluator should see |
|---------:|--------|----------------------|
| 0–10s | Point at the **shape-aware legend** (zones / sites / tracks). Press **Start**, then **4×**. | Idle → running; detections → fused track spheres (lerp). |
| 10–35s | Open **Tracks**: call out `fiber-optic` / RF-dark vs normal swarm. | Same raid, two threat physics — not one jammer blob. |
| 35–55s | When **“Engage high-level threats?”** appears: skim the package list. Click **Y · Engage** *or* **N · Not now**. | Mission-critical effects only after Engage (Operator mode). Soft actions stay on cards. |
| 55–75s | Soft path: Accept a **Cue EO** card (or HUD Cue EO on selected track). | EO joins provenance — recommend, not free-fire. |
| 75–90s | Optional: **Fail radar-N** briefly, then **Restore**. Or toggle **Auto engage** once to show soak-style authorization without the modal. | Tracks coast under fault; Auto log line notes no Y/N. |

**Reset 42** to replay the golden seed. **Random seed** for a different raid layout (same pack). Optional HUD **Class** loads a seeded scenario class over WS (pauses until Start).

## Do say

- Detect → track → recommend → **authorize** (Operator Y/N by default).
- Fiber / RF-dark: soft-kill RF often ineffective; doctrine prefers cue / alert / kinetic — not jammer-first.
- Sim effectors train the decision loop; **Auto engage** is simulated convenience, **not** real autonomous weapons release.

## Do not say

- “This is a real fire-control / autonomous lethal system.”
- “Jammer handles the whole swarm.”
- Architecture digressions (packaging, multi-vertical, Gazebo).

## Verify offline

```bash
./scripts/check.sh
```

Equivalent: `cargo test --workspace`, then `aegis_harness --suite smoke --compare-baseline` and `--assert-golden`.
