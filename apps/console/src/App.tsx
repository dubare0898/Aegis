import { useEffect, useMemo, useRef, useState } from 'react'
import { AirScene } from './AirScene'
import { useSocket } from './useSocket'
import type {
  DefeatEvent,
  DispositionReasonCode,
  EffectorStatus,
  OperatorDisposition,
  OperatorEvent,
  Recommendation,
  Track,
  TrackStatus,
  ZoneState,
} from './types'
import { SCENARIO_CLASSES } from './types'

/** Batch high-threat prompt thresholds (sim seconds / threat score). */
const HIGH_THREAT_SCORE = 48
const HIGH_THREAT_ETA_S = 55
const NOT_NOW_COOLDOWN_S = 12
const THREAT_ESCALATION_DELTA = 8
/** Max open MC accepts per Y / auto package — aligned with open-MC engine cap / effector suite. */
const MAX_BATCH_ENGAGE = 5

type EngageMode = 'operator' | 'auto'

const ACTION_LABEL: Record<string, string> = {
  cue_eo: 'Cue EO/IR',
  alert_sector: 'Alert sector',
  request_jammer_authorization: 'Request jammer auth',
  evacuate_pad: 'Evacuate pad',
  hand_off_higher_echelon: 'Hand off',
  engage_kinetic: 'Engage kinetic',
  maintain_watch: 'Maintain watch',
}

const CLASS_LABEL: Record<string, string> = {
  fixed_wing_uav: 'fixed-wing',
  multirotor: 'multirotor',
  swarm_member: 'swarm',
  fiber_optic_uas: 'fiber-optic',
  manned: 'manned',
  bird: 'bird',
  unknown: 'unknown',
}

const SENSOR_LABEL: Record<string, string> = {
  radar: 'radar',
  rf: 'RF',
  eo_ir: 'EO/IR',
  adsb: 'ADS-B',
  acoustic: 'acoustic',
}

const STATUS_LABEL: Record<string, string> = {
  open: 'open',
  accepted: 'accepted',
  rejected: 'rejected',
  deferred: 'deferred',
}

const TRACK_STATUS_LABEL: Record<TrackStatus, string> = {
  tentative: 'tentative',
  confirmed: 'confirmed',
  coasting: 'coasting',
  dropped: 'dropped',
}

/** Operator-readable zone relative to keep-out / defended asset. */
const ZONE_STATE_LABEL: Record<ZoneState, string> = {
  outside: 'outside',
  watch: 'watch',
  warning: 'keep-out/nofly',
  defended: 'defended asset',
}

/** Schema/engine is source of truth — never re-derive from action enum. */
function needsOperatorAuth(rec: Recommendation): boolean {
  return rec.requires_confirmation === true || rec.criticality === 'mission_critical'
}

function isMissionCriticalAction(action: string): boolean {
  return action === 'request_jammer_authorization' || action === 'engage_kinetic'
}

function formatEta(eta: number | null | undefined): string {
  if (eta == null || Number.isNaN(eta)) return 'outbound'
  if (eta < 60) return `ETA ${eta.toFixed(0)}s`
  return `ETA ${(eta / 60).toFixed(1)}m`
}

function isHighThreatRec(rec: Recommendation, tracks: Track[]): boolean {
  if (!needsOperatorAuth(rec) || !isMissionCriticalAction(rec.action)) return false
  if ((rec.status ?? 'open') !== 'open') return false
  const track = tracks.find((tr) => tr.id === rec.track_id)
  const zone = track?.zone_state
  const zoneHot = zone === 'warning' || zone === 'defended'
  const etaHot = rec.eta_s != null && rec.eta_s <= HIGH_THREAT_ETA_S
  const scoreHot = rec.threat_score >= HIGH_THREAT_SCORE
  return scoreHot || etaHot || zoneHot
}

function sortByThreatThenEta(recs: Recommendation[]): Recommendation[] {
  return [...recs].sort((a, b) => {
    const d = b.threat_score - a.threat_score
    if (Math.abs(d) > 0.5) return d
    return (a.eta_s ?? Number.POSITIVE_INFINITY) - (b.eta_s ?? Number.POSITIVE_INFINITY)
  })
}

function countReadyEffectors(effectors: EffectorStatus[]) {
  let jammer = 0
  let kinetic = 0
  for (const ef of effectors) {
    const ready = !ef.active && ef.cooldown_remaining_s <= 0.05
    if (!ready) continue
    if (ef.kind === 'jammer') jammer += 1
    else if (ef.kind === 'kinetic') kinetic += 1
  }
  return { jammer, kinetic, total: jammer + kinetic }
}

/** Top N open high-threat MC recs, preferring actions that match ready effectors.
 * RF-dark / fiber: never consume jammer slots — kinetic preferred. */
function selectBatchPackage(
  openHigh: Recommendation[],
  effectors: EffectorStatus[],
  tracks: Track[],
): Recommendation[] {
  const sorted = sortByThreatThenEta(openHigh)
  if (sorted.length === 0) return []
  const rfDarkIds = new Set(tracks.filter((tr) => tr.rf_dark).map((tr) => tr.id))
  const ready = countReadyEffectors(effectors)
  const slots = Math.min(
    MAX_BATCH_ENGAGE,
    Math.max(1, ready.total > 0 ? ready.total : Math.min(MAX_BATCH_ENGAGE, sorted.length)),
  )
  const selected: Recommendation[] = []
  let jammerSlots = ready.jammer
  let kineticSlots = ready.kinetic
  const matchReady = ready.total > 0

  // Pass 1: RF-dark → kinetic first (fiber ≠ jammer).
  if (matchReady && kineticSlots > 0) {
    for (const rec of sorted) {
      if (selected.length >= slots || kineticSlots <= 0) break
      if (!rfDarkIds.has(rec.track_id)) continue
      if (rec.action === 'engage_kinetic') {
        selected.push(rec)
        kineticSlots -= 1
      }
    }
  }
  if (matchReady) {
    for (const rec of sorted) {
      if (selected.length >= slots) break
      if (selected.some((s) => s.id === rec.id)) continue
      const dark = rfDarkIds.has(rec.track_id)
      if (rec.action === 'request_jammer_authorization' && jammerSlots > 0 && !dark) {
        selected.push(rec)
        jammerSlots -= 1
      } else if (rec.action === 'engage_kinetic' && kineticSlots > 0) {
        selected.push(rec)
        kineticSlots -= 1
      }
    }
  }
  for (const rec of sorted) {
    if (selected.length >= slots) break
    if (selected.some((s) => s.id === rec.id)) continue
    // Never auto-fill jammer for RF-dark even if slots remain unmatched.
    if (rec.action === 'request_jammer_authorization' && rfDarkIds.has(rec.track_id)) continue
    selected.push(rec)
  }
  return selected
}

function packageFingerprint(recs: Recommendation[]): string {
  return sortByThreatThenEta(recs)
    .map((r) => r.id)
    .join('|')
}

type LocalLogEntry = {
  t: number
  message: string
}

type PendingEngage =
  | { mode: 'batch'; package: Recommendation[]; rfDarkCount: number }
  | { mode: 'single'; rec: Recommendation; rfDark: boolean }

export default function App() {
  const { picture, connected, send } = useSocket()
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [showTruth, setShowTruth] = useState(false)
  /** Default Operator (Y/N) — safer for demos; Auto engages without modal. */
  const [engageMode, setEngageMode] = useState<EngageMode>('operator')
  const [pendingEngage, setPendingEngage] = useState<PendingEngage | null>(null)
  const [localLog, setLocalLog] = useState<LocalLogEntry[]>([])
  /** Single-rec prompts the operator dismissed/declined (avoid re-spam). */
  const skippedPromptIds = useRef(new Set<string>())
  /** Batch "Not now" cooldown in sim time. */
  const notNowUntilSimT = useRef(0)
  const notNowMaxThreat = useRef(0)
  const notNowTrackIds = useRef(new Set<string>())
  const lastBatchFingerprint = useRef('')
  /** Rec ids already auto-accepted this cycle (avoid re-spam). */
  const autoTaskedIds = useRef(new Set<string>())

  const tracks = useMemo(() => {
    const list = picture?.tracks ?? []
    return [...list].sort((a, b) => b.threat_score - a.threat_score)
  }, [picture])

  const recs = picture?.recommendations ?? []
  const serverEvents = [...(picture?.operator_events ?? [])].reverse().slice(0, 12)
  const effectors = picture?.effectors ?? []
  const defeatEvents = picture?.defeat_events ?? []
  const downedCounts = useMemo(() => countDefeats(defeatEvents), [defeatEvents])
  const alertedZones = picture?.operator_state?.alerted_zone_ids ?? []
  const padEvacuated = Boolean(picture?.operator_state?.evacuated_pad)
  const simT = picture?.t ?? 0
  const phaseLine = !picture
    ? 'Connecting…'
    : !picture.running
      ? 'Idle — press Start'
      : engageMode === 'auto'
        ? 'AUTO ENGAGE — high-threat package authorized without Y/N'
        : pendingEngage
          ? 'Authorize — high-threat package pending'
          : tracks.length === 0
            ? 'Detecting…'
            : 'Tracking — recommend & authorize'

  const pushLocalLog = (message: string) => {
    setLocalLog((prev) => [...prev, { t: simT, message }].slice(-24))
  }

  const clearPromptState = () => {
    skippedPromptIds.current.clear()
    notNowUntilSimT.current = 0
    notNowMaxThreat.current = 0
    notNowTrackIds.current.clear()
    lastBatchFingerprint.current = ''
    autoTaskedIds.current.clear()
    setPendingEngage(null)
    setLocalLog([])
  }

  const openSingleEngagePrompt = (rec: Recommendation) => {
    const track = tracks.find((tr) => tr.id === rec.track_id)
    setPendingEngage({
      mode: 'single',
      rec,
      rfDark: Boolean(track?.rf_dark),
    })
    setSelectedId(rec.track_id)
    pushLocalLog(
      `Accept held — confirm engage for ${ACTION_LABEL[rec.action] ?? rec.action} on ${rec.track_id.slice(0, 8)}`,
    )
  }

  const cueSelected = () => {
    if (selectedId) send({ type: 'cue_eo', track_id: selectedId })
  }

  const sendDispose = (
    recommendationId: string,
    disposition: OperatorDisposition,
    reason?: DispositionReasonCode,
  ) => {
    send({
      type: 'dispose_recommendation',
      recommendation_id: recommendationId,
      disposition,
      actor: 'operator',
      reason_code: reason,
    })
  }

  const authorizePackage = (pkg: Recommendation[], source: 'auto' | 'operator') => {
    for (const rec of pkg) {
      skippedPromptIds.current.add(rec.id)
      if (source === 'auto') autoTaskedIds.current.add(rec.id)
      sendDispose(rec.id, 'accepted', 'approved_best_option')
    }
    notNowUntilSimT.current = 0
    notNowMaxThreat.current = 0
    notNowTrackIds.current.clear()
    lastBatchFingerprint.current = packageFingerprint(pkg)
    const label = pkg.map((r) => ACTION_LABEL[r.action] ?? r.action).join(', ')
    pushLocalLog(
      source === 'auto'
        ? `AUTO ENGAGE — authorized ${pkg.length} high-threat action(s): ${label}`
        : `ENGAGE Y — authorized ${pkg.length} high-threat action(s): ${label}`,
    )
  }

  // Operator batch-prompt OR auto-accept when high-level open MC threats appear.
  useEffect(() => {
    if (engageMode === 'operator' && pendingEngage) return
    const openHigh = recs.filter(
      (r) =>
        isHighThreatRec(r, tracks) &&
        !skippedPromptIds.current.has(r.id) &&
        !autoTaskedIds.current.has(r.id),
    )
    if (openHigh.length === 0) return

    const pkg = selectBatchPackage(openHigh, effectors, tracks)
    if (pkg.length === 0) return

    if (engageMode === 'auto') {
      const fresh = pkg.filter((r) => !autoTaskedIds.current.has(r.id))
      if (fresh.length === 0) return
      const fp = packageFingerprint(fresh)
      if (fp === lastBatchFingerprint.current) return
      setSelectedId(fresh[0]?.track_id ?? null)
      authorizePackage(fresh, 'auto')
      return
    }

    const maxThreat = Math.max(...pkg.map((r) => r.threat_score))
    const newHigher =
      pkg.some((r) => !notNowTrackIds.current.has(r.track_id)) ||
      maxThreat >= notNowMaxThreat.current + THREAT_ESCALATION_DELTA
    if (simT < notNowUntilSimT.current && !newHigher) return

    const fp = packageFingerprint(pkg)
    // Same package already shown this cycle — skip until N-cooldown clears fingerprint.
    if (fp === lastBatchFingerprint.current) return

    const rfDarkCount = pkg.filter((r) =>
      tracks.some((tr) => tr.id === r.track_id && tr.rf_dark),
    ).length
    lastBatchFingerprint.current = fp
    setPendingEngage({ mode: 'batch', package: pkg, rfDarkCount })
    setSelectedId(pkg[0]?.track_id ?? null)
    pushLocalLog(
      `High-threat prompt — ${pkg.length} mission-critical action(s) awaiting Engage Y/N`,
    )
    // eslint-disable-next-line react-hooks/exhaustive-deps -- open when rec/effector picture changes
  }, [recs, pendingEngage, effectors, tracks, simT, engageMode])

  const onDispose = (rec: Recommendation, disposition: OperatorDisposition) => {
    if (disposition === 'accepted' && needsOperatorAuth(rec)) {
      if (engageMode === 'auto') {
        skippedPromptIds.current.add(rec.id)
        autoTaskedIds.current.add(rec.id)
        sendDispose(rec.id, 'accepted', 'approved_best_option')
        pushLocalLog(
          `AUTO ENGAGE — ${ACTION_LABEL[rec.action] ?? rec.action} on ${rec.track_id.slice(0, 8)}`,
        )
        return
      }
      openSingleEngagePrompt(rec)
      return
    }
    const reason: DispositionReasonCode | undefined =
      disposition === 'accepted'
        ? 'approved_best_option'
        : disposition === 'rejected'
          ? 'false_positive_suspected'
          : 'waiting_for_higher'
    sendDispose(rec.id, disposition, reason)
  }

  const confirmEngage = () => {
    if (!pendingEngage) return
    if (pendingEngage.mode === 'batch') {
      authorizePackage(pendingEngage.package, 'operator')
      setPendingEngage(null)
      return
    }
    const { rec } = pendingEngage
    skippedPromptIds.current.add(rec.id)
    pushLocalLog(
      `ENGAGE confirmed — ${ACTION_LABEL[rec.action] ?? rec.action} on ${rec.track_id.slice(0, 8)}`,
    )
    sendDispose(rec.id, 'accepted', 'approved_best_option')
    setPendingEngage(null)
  }

  const declineEngage = () => {
    if (!pendingEngage) return
    if (pendingEngage.mode === 'batch') {
      const pkg = pendingEngage.package
      const maxThreat = Math.max(...pkg.map((r) => r.threat_score), 0)
      notNowUntilSimT.current = simT + NOT_NOW_COOLDOWN_S
      notNowMaxThreat.current = maxThreat
      notNowTrackIds.current = new Set(pkg.map((r) => r.track_id))
      // Clear fingerprint so the same package can reappear after cooldown / escalation.
      lastBatchFingerprint.current = ''
      pushLocalLog(
        `Engage N — not now; re-prompt after ~${NOT_NOW_COOLDOWN_S}s or higher threat`,
      )
      setPendingEngage(null)
      return
    }
    const { rec } = pendingEngage
    skippedPromptIds.current.add(rec.id)
    pushLocalLog(
      `Engage declined (not now) — ${ACTION_LABEL[rec.action] ?? rec.action} deferred`,
    )
    sendDispose(rec.id, 'deferred', 'waiting_for_higher')
    setPendingEngage(null)
  }

  const dismissPrompt = () => {
    if (!pendingEngage) return
    if (pendingEngage.mode === 'batch') {
      // Soft dismiss: short cooldown, no disposition (fine-control cards still work).
      notNowUntilSimT.current = simT + Math.min(6, NOT_NOW_COOLDOWN_S)
      notNowMaxThreat.current = Math.max(
        ...pendingEngage.package.map((r) => r.threat_score),
        0,
      )
      notNowTrackIds.current = new Set(pendingEngage.package.map((r) => r.track_id))
      lastBatchFingerprint.current = ''
      pushLocalLog(`Engage prompt dismissed — no effector change`)
      setPendingEngage(null)
      return
    }
    const { rec } = pendingEngage
    skippedPromptIds.current.add(rec.id)
    pushLocalLog(`Engage prompt dismissed — no effector change`)
    setPendingEngage(null)
  }

  // Keyboard Y / N while prompt is open.
  useEffect(() => {
    if (!pendingEngage) return
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return
      const key = e.key.toLowerCase()
      if (key === 'y' || key === 'enter') {
        e.preventDefault()
        confirmEngage()
      } else if (key === 'n' || key === 'escape') {
        e.preventDefault()
        if (key === 'escape') dismissPrompt()
        else declineEngage()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- close over latest handlers/pending
  }, [pendingEngage])

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          AEGIS <span>C-UAS</span>
        </div>
        <div className="meta">
          <div>
            link <strong>{connected ? 'live' : 'reconnecting'}</strong>
          </div>
          <div>
            scenario <strong>{picture?.scenario_id ?? '—'}</strong>
          </div>
          <div>
            t <strong>{picture ? picture.t.toFixed(1) : '—'}s</strong>
          </div>
          <div>
            seed <strong>{picture?.seed ?? '—'}</strong>
          </div>
          <div>
            tracks <strong>{picture?.tracks.length ?? 0}</strong>
          </div>
          <div>
            sim <strong>{picture?.running ? 'running' : 'idle'}</strong>
          </div>
          {padEvacuated && (
            <div>
              pad <strong>evacuated</strong>
            </div>
          )}
        </div>
      </header>

      <div className="stage">
        <div className="stage-main">
          <div className="overlay-title">
            <h1>Aegis · FOB Sentinel</h1>
            <p className="phase-line">{phaseLine}</p>
            <div className="legend" style={{ marginTop: '0.55rem' }}>
              <div className="legend-group">
                <span className="legend-label">zones</span>
                <span><i className="glyph ring zone-critical" aria-hidden /> critical</span>
                <span><i className="glyph ring zone-keepout" aria-hidden /> keep-out</span>
                <span><i className="glyph ring zone-surv" aria-hidden /> surveillance</span>
                <span><i className="glyph hex asset" aria-hidden /> defended asset</span>
              </div>
              <div className="legend-group">
                <span className="legend-label">sites</span>
                <span><i className="glyph tri sensor" aria-hidden /> sensor</span>
                <span><i className="glyph tri sensor-failed" aria-hidden /> failed</span>
                <span><i className="glyph diamond jammer" aria-hidden /> jammer</span>
                <span><i className="glyph diamond kinetic" aria-hidden /> kinetic</span>
              </div>
              <div className="legend-group">
                <span className="legend-label">tracks</span>
                <span><i className="glyph sphere selected" aria-hidden /> selected</span>
                <span><i className="glyph sphere friendly" aria-hidden /> friendly</span>
                <span><i className="glyph sphere rf-dark" aria-hidden /> RF-dark</span>
                <span><i className="glyph sphere rf-dark-warn" aria-hidden /> RF-dark warn</span>
                <span><i className="glyph sphere rf-dark-def" aria-hidden /> RF-dark def</span>
                <span><i className="glyph sphere hostile-low" aria-hidden /> low</span>
                <span><i className="glyph sphere hostile-mid" aria-hidden /> mid</span>
                <span><i className="glyph sphere hostile" aria-hidden /> high</span>
                <span><i className="glyph sphere hostile-warn" aria-hidden /> warning</span>
                <span><i className="glyph sphere hostile-def" aria-hidden /> defended</span>
              </div>
              {showTruth && (
                <div className="legend-group">
                  <span className="legend-label">truth</span>
                  <span><i className="glyph wire-diamond truth" aria-hidden /> live</span>
                  <span><i className="glyph wire-diamond truth-jammed" aria-hidden /> jammed</span>
                  <span><i className="glyph wire-diamond truth-dead" aria-hidden /> neutralized</span>
                </div>
              )}
            </div>
          </div>
          <AirScene
            picture={picture}
            selectedId={selectedId}
            showTruth={showTruth}
            alertedZoneIds={alertedZones}
            onSelectTrack={setSelectedId}
          />
          {engageMode === 'operator' && pendingEngage && (
            <EngageConfirmModal
              pending={pendingEngage}
              onEngage={confirmEngage}
              onNotNow={declineEngage}
              onDismiss={dismissPrompt}
            />
          )}
          <div className="hud">
            <button className="primary" onClick={() => send({ type: 'start' })}>
              Start
            </button>
            <button onClick={() => send({ type: 'pause' })}>Pause</button>
            <button onClick={() => send({ type: 'set_speed', speed: 1 })}>1×</button>
            <button onClick={() => send({ type: 'set_speed', speed: 4 })}>4×</button>
            <button
              onClick={() => {
                clearPromptState()
                send({ type: 'reset', seed: 42 })
              }}
            >
              Reset 42
            </button>
            <button
              onClick={() => {
                clearPromptState()
                // Safe integer for JSON number transport (ClientCommand::Reset seed).
                const u = new Uint32Array(2)
                crypto.getRandomValues(u)
                const seed = (u[0]! & 0x1fffff) * 0x100000000 + u[1]!
                send({ type: 'reset', seed })
              }}
            >
              Random seed
            </button>
            <label className="hud-class">
              <span className="hud-class-label">Class</span>
              <select
                value={picture?.scenario_class ?? ''}
                title="Load a seeded scenario class (pauses until Start)"
                onChange={(e) => {
                  const cls = e.target.value
                  if (!cls) return
                  clearPromptState()
                  send({ type: 'set_scenario_class', class: cls, seed: picture?.seed ?? 42 })
                  pushLocalLog(`Scenario class — ${cls} (seed ${picture?.seed ?? 42})`)
                }}
              >
                <option value="">site pack</option>
                {SCENARIO_CLASSES.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.label}
                  </option>
                ))}
              </select>
            </label>
            <button onClick={cueSelected} disabled={!selectedId}>
              Cue EO
            </button>
            <button onClick={() => send({ type: 'fail_sensor', sensor_id: 'radar-north' })}>
              Fail radar-N
            </button>
            <button onClick={() => send({ type: 'restore_sensor', sensor_id: 'radar-north' })}>
              Restore radar-N
            </button>
            <button
              className={`hud-debug ${showTruth ? 'primary' : ''}`}
              onClick={() => setShowTruth((v) => !v)}
              title="Debug: show ground truth"
            >
              Debug truth
            </button>
            <div className="hud-mode" role="group" aria-label="Engage mode">
              <button
                type="button"
                className={engageMode === 'operator' ? 'primary' : ''}
                onClick={() => {
                  setEngageMode('operator')
                  pushLocalLog('Engage mode — Operator (Y/N)')
                }}
                title="Operator confirms jammer/kinetic with Y/N"
              >
                Operator (Y/N)
              </button>
              <button
                type="button"
                className={engageMode === 'auto' ? 'primary hud-auto' : ''}
                onClick={() => {
                  setEngageMode('auto')
                  setPendingEngage(null)
                  lastBatchFingerprint.current = ''
                  pushLocalLog('Engage mode — AUTO ENGAGE (no Y/N modal)')
                }}
                title="Sim-only: auto-accept high-threat jammer/kinetic package"
              >
                Auto engage
              </button>
            </div>
            <div className="hud-state" aria-live="polite">
              <span className={engageMode === 'auto' ? 'hud-auto-badge' : undefined}>
                mode:{' '}
                <strong>{engageMode === 'auto' ? 'AUTO ENGAGE' : 'Operator (Y/N)'}</strong>
              </span>
              <span>
                zones alerted:{' '}
                <strong>
                  {alertedZones.length > 0 ? alertedZones.join(', ') : 'none'}
                </strong>
              </span>
              <span className={padEvacuated ? 'hud-evac' : undefined}>
                pad:{' '}
                <strong>{padEvacuated ? 'evacuated' : 'clear'}</strong>
              </span>
            </div>
          </div>
        </div>

        <aside className="rail">
          <section>
            <h2>Recommendations</h2>
            {recs.length === 0 && (
              <p style={{ color: 'var(--muted)', fontSize: '0.72rem' }}>
                Waiting for ranked threats…
              </p>
            )}
            {recs.map((r) => (
              <RecCard
                key={r.id}
                rec={r}
                onFocus={() => setSelectedId(r.track_id)}
                onDispose={onDispose}
              />
            ))}
          </section>
          <section>
            <h2>Operator log</h2>
            {localLog.length === 0 && serverEvents.length === 0 && (
              <p style={{ color: 'var(--muted)', fontSize: '0.72rem' }}>
                Accept / Reject / Defer a recommendation to record decisions.
              </p>
            )}
            {[...localLog].reverse().slice(0, 8).map((e, i) => (
              <div key={`local-${e.t}-${i}`} className="event-row">
                <div className="meta-line">
                  <strong>t={e.t.toFixed(1)} · prompt</strong>
                  <span>local</span>
                </div>
                <div style={{ color: 'var(--muted)', fontSize: '0.62rem' }}>{e.message}</div>
              </div>
            ))}
            {serverEvents.map((e) => (
              <EventRow key={`${e.recommendation_id}-${e.t}-${e.disposition}`} event={e} />
            ))}
          </section>
          <section>
            <h2>Enemies downed</h2>
            <div className="downed-counters">
              <span>
                Total <strong>{downedCounts.total}</strong>
              </span>
              <span>
                Kinetic <strong>{downedCounts.kinetic}</strong>
              </span>
              <span>
                Jamming <strong>{downedCounts.jamming}</strong>
              </span>
              <span>
                RF <strong>{downedCounts.rf}</strong>
              </span>
            </div>
            {defeatEvents.length === 0 && (
              <p style={{ color: 'var(--muted)', fontSize: '0.72rem' }}>
                No defeats yet — authorize jammer or kinetic engagements.
              </p>
            )}
            {[...defeatEvents].reverse().slice(0, 12).map((e) => (
              <DefeatRow key={`${e.cause}-${e.truth_id}-${e.t}`} event={e} />
            ))}
          </section>
          <section>
            <h2>Effectors</h2>
            {effectors.length === 0 && (
              <p style={{ color: 'var(--muted)', fontSize: '0.72rem' }}>No effectors in scenario</p>
            )}
            {effectors.map((ef) => (
              <div key={ef.id} className="track-row" style={{ cursor: 'default' }}>
                <div>
                  {ef.id} · {ef.kind}
                  <div style={{ color: 'var(--muted)', fontSize: '0.62rem' }}>
                    {ef.last_result ?? `range ${ef.range_m.toFixed(0)} m`}
                    {ef.cooldown_remaining_s > 0.05
                      ? ` · cd ${ef.cooldown_remaining_s.toFixed(1)}s`
                      : ''}
                  </div>
                </div>
                <div style={{ color: ef.active ? 'var(--warn)' : 'var(--muted)' }}>
                  {ef.active ? 'live' : 'idle'}
                </div>
              </div>
            ))}
          </section>
          <section>
            <h2>Tracks</h2>
            {tracks.map((tr) => (
              <TrackRow
                key={tr.id}
                track={tr}
                active={selectedId === tr.id}
                onSelect={() => setSelectedId(tr.id)}
                outcome={trackOutcome(tr.id, effectors, defeatEvents, serverEvents)}
              />
            ))}
          </section>
          <section>
            <h2>Sensors</h2>
            {(picture?.sensors ?? []).map((s) => (
              <div key={s.id} className="track-row" style={{ cursor: 'default' }}>
                <div>
                  {s.id} · {SENSOR_LABEL[s.kind] ?? s.kind}
                  <div style={{ color: 'var(--muted)', fontSize: '0.62rem' }}>
                    range {s.range_m.toFixed(0)} m
                    {s.tasked_track_id ? ` · tasked ${s.tasked_track_id.slice(0, 8)}` : ''}
                  </div>
                </div>
                <div style={{ color: s.healthy ? 'var(--accent)' : 'var(--danger)' }}>
                  {s.healthy ? 'up' : 'down'}
                </div>
              </div>
            ))}
          </section>
        </aside>
      </div>
    </div>
  )
}

function EngageConfirmModal({
  pending,
  onEngage,
  onNotNow,
  onDismiss,
}: {
  pending: PendingEngage
  onEngage: () => void
  onNotNow: () => void
  onDismiss: () => void
}) {
  if (pending.mode === 'single') {
    const { rec, rfDark } = pending
    return (
      <div className="engage-modal-backdrop" role="dialog" aria-modal="true">
        <div className="engage-modal">
          <p className="engage-kicker">Mission-critical decision</p>
          <h2>Engage this threat?</h2>
          <p className="engage-context">
            Proposed: <strong>{ACTION_LABEL[rec.action] ?? rec.action}</strong>
            {' · '}
            track {rec.track_id.slice(0, 8)}
            {' · '}
            threat {rec.threat_score.toFixed(0)}
            {' · '}
            {formatEta(rec.eta_s)}
            {rfDark ? ' · RF-dark (jammer likely ineffective)' : ''}
          </p>
          {rec.rank_rationale && (
            <p className="engage-detail" style={{ marginTop: 0 }}>
              Rank: {rec.rank_rationale}
            </p>
          )}
          <p className="engage-detail">{rec.title}</p>
          <div className="engage-actions">
            <button className="primary danger-btn" onClick={onEngage}>
              Y · Engage
            </button>
            <button onClick={onNotNow}>N · Not now</button>
            <button onClick={onDismiss}>Dismiss</button>
          </div>
        </div>
      </div>
    )
  }

  const { package: pkg, rfDarkCount } = pending
  const top = pkg.slice(0, 4)
  return (
    <div className="engage-modal-backdrop" role="dialog" aria-modal="true">
      <div className="engage-modal">
        <p className="engage-kicker">High-threat package · {pkg.length} track(s)</p>
        <h2>Engage high-level threats?</h2>
        <p className="engage-context">
          Authorizes the current top jammer/kinetic package — not free-fire.
          {rfDarkCount > 0
            ? ` · ${rfDarkCount} RF-dark (jammer likely ineffective; kinetic preferred)`
            : ''}
        </p>
        <ul className="engage-package-list">
          {top.map((rec) => (
            <li key={rec.id}>
              <strong>{ACTION_LABEL[rec.action] ?? rec.action}</strong>
              {' · '}
              {rec.track_id.slice(0, 8)}
              {' · '}
              threat {rec.threat_score.toFixed(0)}
              {' · '}
              {formatEta(rec.eta_s)}
            </li>
          ))}
        </ul>
        <div className="engage-actions">
          <button className="primary danger-btn" onClick={onEngage}>
            Y · Engage
          </button>
          <button onClick={onNotNow}>N · Not now</button>
          <button onClick={onDismiss}>Dismiss</button>
        </div>
      </div>
    </div>
  )
}

function countDefeats(events: DefeatEvent[]) {
  const ids = new Set<string>()
  const kinetic = new Set<string>()
  const jamming = new Set<string>()
  const rf = new Set<string>()
  for (const e of events) {
    ids.add(e.truth_id)
    if (e.cause === 'kinetic') kinetic.add(e.truth_id)
    if (e.cause === 'jamming') jamming.add(e.truth_id)
    if (!e.rf_dark) rf.add(e.truth_id)
  }
  return {
    total: ids.size,
    kinetic: kinetic.size,
    jamming: jamming.size,
    rf: rf.size,
  }
}

function DefeatRow({ event }: { event: DefeatEvent }) {
  const causeLabel = event.cause === 'kinetic' ? 'KINETIC' : 'JAMMING'
  return (
    <div className="event-row">
      <div className="meta-line">
        <strong>
          t={event.t.toFixed(1)} · {causeLabel}
        </strong>
        <span>{event.label}</span>
      </div>
      <div style={{ color: 'var(--muted)', fontSize: '0.62rem' }}>
        {event.rf_dark ? 'RF-dark · ' : 'RF · '}
        {event.note || (event.cause === 'kinetic' ? 'neutralized' : 'C2 suppressed')}
      </div>
    </div>
  )
}

function trackOutcome(
  trackId: string,
  effectors: EffectorStatus[],
  defeats: DefeatEvent[],
  events: OperatorEvent[],
): string | null {
  const tasked = effectors.find((e) => e.tasked_track_id === trackId && e.last_result)
  if (tasked?.last_result) {
    return `${tasked.kind}: ${tasked.last_result}`
  }
  const ev = events.find((e) => e.track_id === trackId && e.disposition === 'accepted')
  if (ev?.note) return ev.note
  // Defeat events are keyed by truth id; show nearest recent note when track was engaged.
  const recent = [...defeats].reverse().find((d) => d.note && d.t > 0)
  if (recent && tasked) return `${recent.cause}: ${recent.note || 'effect applied'}`
  return null
}

function TrackRow({
  track: tr,
  active,
  onSelect,
  outcome,
}: {
  track: Track
  active: boolean
  onSelect: () => void
  outcome: string | null
}) {
  const status = tr.track_status ?? 'tentative'
  const zone = tr.zone_state ?? 'outside'
  const zoneHot = zone === 'warning' || zone === 'defended'
  return (
    <div
      className={`track-row ${active ? 'active' : ''}`}
      onClick={onSelect}
    >
      <div>
        {tr.id.slice(0, 8)} ·{' '}
        {CLASS_LABEL[tr.class_hypothesis] ?? tr.class_hypothesis}
        {tr.rf_dark ? ' · RF-dark' : ''} · {tr.affiliation}
        <div className="track-meta-chips">
          <span className="status-chip">{TRACK_STATUS_LABEL[status]}</span>
          <span className={`status-chip ${zoneHot ? 'zone-hot' : ''}`}>
            {ZONE_STATE_LABEL[zone]}
          </span>
        </div>
        <div style={{ color: 'var(--muted)', fontSize: '0.62rem' }}>
          {tr.sensor_provenance.map((k) => SENSOR_LABEL[k] ?? k).join('+') || '—'} · hits{' '}
          {tr.hit_count} · {formatEta(tr.eta_s)}
        </div>
        {outcome && (
          <div className="track-outcome" style={{ color: 'var(--warn)', fontSize: '0.62rem' }}>
            Outcome: {outcome}
          </div>
        )}
      </div>
      <div className="score">{tr.threat_score.toFixed(0)}</div>
    </div>
  )
}

function RecCard({
  rec,
  onFocus,
  onDispose,
}: {
  rec: Recommendation
  onFocus: () => void
  onDispose: (rec: Recommendation, d: OperatorDisposition) => void
}) {
  const critical = needsOperatorAuth(rec)
  const status = rec.status ?? 'open'
  const open = status === 'open'
  const evidence = (rec.evidence ?? []).slice(0, 4)
  return (
    <article className={`rec-card ${critical ? 'danger' : ''}`} onClick={onFocus}>
      <div className="meta-line">
        <strong>
          P{rec.priority} · {ACTION_LABEL[rec.action] ?? rec.action}
        </strong>
        <span className={`status-chip status-${status}`}>
          {STATUS_LABEL[status] ?? status}
        </span>
      </div>
      <div className="meta-line" style={{ marginTop: '0.25rem' }}>
        <span className={`crit-chip ${critical ? 'crit-hard' : 'crit-soft'}`}>
          {critical ? 'mission-critical' : 'soft'}
        </span>
        <span>
          {critical ? 'auth required' : 'no auth gate'} · {(rec.confidence * 100).toFixed(0)}%
        </span>
      </div>
      <h3>{rec.title}</h3>
      <p className="rec-effect">
        {formatEta(rec.eta_s)}
        {rec.rank_rationale ? ` · ${rec.rank_rationale}` : ''}
      </p>
      {rec.expected_effect && (
        <p className="rec-effect">
          If accepted: {rec.expected_effect}
        </p>
      )}
      {critical && open && (rec.blocked_by?.length ?? 0) > 0 && (
        <p className="rec-blocked">
          Blocked pending: {rec.blocked_by?.join(', ')}
        </p>
      )}
      {evidence.length > 0 && (
        <p className="rec-evidence">
          Evidence:{' '}
          {evidence.map((e) => `${e.kind}=${e.value}`).join(' · ')}
        </p>
      )}
      <div className="meta-line" style={{ marginTop: '0.35rem' }}>
        <span>{rec.uncertainty}</span>
        <span>threat {rec.threat_score.toFixed(0)}</span>
      </div>
      <div className="rec-actions" onClick={(e) => e.stopPropagation()}>
        <button
          className="primary"
          disabled={!open}
          onClick={() => onDispose(rec, 'accepted')}
        >
          {critical ? 'Review' : 'Accept'}
        </button>
        <button disabled={!open} onClick={() => onDispose(rec, 'rejected')}>
          Reject
        </button>
        <button disabled={!open} onClick={() => onDispose(rec, 'deferred')}>
          Defer
        </button>
      </div>
    </article>
  )
}

function EventRow({ event }: { event: OperatorEvent }) {
  const actor = event.actor ?? 'operator'
  return (
    <div className="event-row">
      <div className="meta-line">
        <strong>
          t={event.t.toFixed(1)} · {event.disposition}
        </strong>
        <span>{ACTION_LABEL[event.action] ?? event.action}</span>
      </div>
      <div className="event-tags">
        <span className="status-chip actor-chip">{actor}</span>
        {event.reason_code && (
          <span className="status-chip reason-chip">{event.reason_code}</span>
        )}
      </div>
      <div style={{ color: 'var(--muted)', fontSize: '0.62rem' }}>
        track {event.track_id.slice(0, 8)}
        {event.note ? ` — ${event.note}` : ''}
      </div>
    </div>
  )
}
