import { useEffect, useMemo, useRef, useState } from 'react'
import { AirScene } from './AirScene'
import { useSocket } from './useSocket'
import type {
  DefeatEvent,
  DispositionReasonCode,
  OperatorDisposition,
  OperatorEvent,
  Recommendation,
  Track,
  TrackStatus,
  ZoneState,
} from './types'

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

function formatEta(eta: number | null | undefined): string {
  if (eta == null || Number.isNaN(eta)) return 'outbound'
  if (eta < 60) return `ETA ${eta.toFixed(0)}s`
  return `ETA ${(eta / 60).toFixed(1)}m`
}

type LocalLogEntry = {
  t: number
  message: string
}

type PendingEngage = {
  rec: Recommendation
  rfDark: boolean
}

export default function App() {
  const { picture, connected, send } = useSocket()
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [showTruth, setShowTruth] = useState(false)
  const [pendingEngage, setPendingEngage] = useState<PendingEngage | null>(null)
  const [localLog, setLocalLog] = useState<LocalLogEntry[]>([])
  const skippedPromptIds = useRef(new Set<string>())

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
      : pendingEngage
        ? 'Authorize — mission-critical decision pending'
        : tracks.length === 0
          ? 'Detecting…'
          : 'Tracking — recommend & authorize'

  const pushLocalLog = (message: string) => {
    setLocalLog((prev) => [...prev, { t: simT, message }].slice(-24))
  }

  const openEngagePrompt = (rec: Recommendation, source: 'auto' | 'accept') => {
    const track = tracks.find((tr) => tr.id === rec.track_id)
    setPendingEngage({ rec, rfDark: Boolean(track?.rf_dark) })
    setSelectedId(rec.track_id)
    if (source === 'auto') {
      pushLocalLog(
        `Swarm prompt — ${ACTION_LABEL[rec.action] ?? rec.action} on ${rec.track_id.slice(0, 8)} (awaiting operator)`,
      )
    } else {
      pushLocalLog(
        `Accept held — confirm engage for ${ACTION_LABEL[rec.action] ?? rec.action} on ${rec.track_id.slice(0, 8)}`,
      )
    }
  }

  // Auto-prompt when engine marks an open rec as requiring confirmation.
  useEffect(() => {
    if (pendingEngage) return
    const critical = recs.find(
      (r) =>
        needsOperatorAuth(r) &&
        (r.status ?? 'open') === 'open' &&
        !skippedPromptIds.current.has(r.id),
    )
    if (critical) {
      openEngagePrompt(critical, 'auto')
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- open only when rec set changes
  }, [recs, pendingEngage])

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

  const onDispose = (rec: Recommendation, disposition: OperatorDisposition) => {
    if (disposition === 'accepted' && needsOperatorAuth(rec)) {
      openEngagePrompt(rec, 'accept')
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
    const { rec } = pendingEngage
    skippedPromptIds.current.add(rec.id)
    pushLocalLog(`Engage prompt dismissed — no effector change`)
    setPendingEngage(null)
  }

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
              <span><i className="dot hostile" /> track</span>
              <span><i className="dot friendly" /> friendly</span>
              <span><i className="dot sensor" /> sensor</span>
              <span><i className="dot effector" /> effector</span>
              <span><i className="dot asset" /> defended asset</span>
              {showTruth && <span><i className="dot truth" /> truth (debug)</span>}
            </div>
          </div>
          <AirScene
            picture={picture}
            selectedId={selectedId}
            showTruth={showTruth}
            alertedZoneIds={alertedZones}
            onSelectTrack={setSelectedId}
          />
          {pendingEngage && (
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
                skippedPromptIds.current.clear()
                setPendingEngage(null)
                setLocalLog([])
                send({ type: 'reset', seed: 42 })
              }}
            >
              Reset 42
            </button>
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
            <div className="hud-state" aria-live="polite">
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
  const { rec, rfDark } = pending
  return (
    <div className="engage-modal-backdrop" role="dialog" aria-modal="true">
      <div className="engage-modal">
        <p className="engage-kicker">Mission-critical decision</p>
        <h2>Swarm detected. Do you want to engage defenses?</h2>
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
            Engage
          </button>
          <button onClick={onNotNow}>Not now</button>
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

function TrackRow({
  track: tr,
  active,
  onSelect,
}: {
  track: Track
  active: boolean
  onSelect: () => void
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
