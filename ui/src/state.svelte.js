import { fmt, leaf, summarize } from './api.js'

// Trust flags (docs/api.md, "Trust flags"): the stages that did not reach their outcome for
// this event, read from the fields the emitted line already carries. Nothing is computed on
// the engine's hot path for this and nothing is a probability: summing one flag over the
// output equals the counter block's counter of the same name.
export function flagsOf(l) {
  const u = l?.ulpf ?? {}
  const f = []
  if (u.parse_status === 'no_parser') f.push('no_parser')
  else if (u.parse_status && u.parse_status !== 'parsed') f.push(`parse_failed:${u.parse_status}`)
  if (u.sub_status === 'uncovered') f.push('sub_uncovered')
  if (u.sub_status === 'no_match') f.push('sub_no_match')
  if (Array.isArray(u.time_policies) && u.time_policies.includes('receipt_fallback')) f.push('time_from_receipt')
  if (u.time_error) f.push(`time_error:${u.time_error}`)
  if (l?.class_uid === 0) f.push('class_unknown')
  const n = l?.unmapped ? Object.keys(l.unmapped).length : 0
  if (n) f.push(`unmapped:${n}`)
  if (u.utf8_lossy === true) f.push('utf8_lossy')
  return f
}

// A tail row is flattened the moment it arrives: ten fields (eight strings, the flag
// list and the raw id), never the nested event.
// Keeping the whole normalized object in reactive state would proxy every nested field of
// every row on every frame, which is what locks a browser at full rate. `text` is the whole
// line once, lowercased, so the filter is a substring test per term and not a walk per field.
export function row(ev) {
  const l = ev.line
  return {
    flags: flagsOf(l),
    // ponytail: the first 64 KiB of the line. A 4 MB single-line record kept whole would put
    // 2 GB in a full tail; a term past 64 KiB still matches in the export, which reads the file.
    text: JSON.stringify(l ?? null).slice(0, 65536).toLowerCase(),
    raw_id: ev.raw_id,
    time: fmt.stamp(leaf(l, 'metadata.event_time_rfc3339') ?? l?.time),
    parser: leaf(l, 'ulpf.parser') ?? null,
    status: leaf(l, 'ulpf.parse_status') ?? 'unknown',
    cls: l?.class_name ?? '',
    action: l?.action ?? '',
    device: leaf(l, 'device.hostname') ?? leaf(l, 'metadata.log_name') ?? '',
    sum: summarize(l),
  }
}

// Live state fed by GET /api/stream (hello, metrics, tail, pending, drift, integrity, replay).
// Frames are applied on requestAnimationFrame: a frame that arrives before the previous one
// was painted replaces it and is counted in `dropped`. Nothing queues, so a full-rate engine
// cannot outrun the browser.
export const live = $state({
  conn: 'connecting', // connecting | live | reconnecting
  retryIn: 0,
  status: null,
  metrics: null,
  tail: [],
  paused: false,
  held: 0, // events buffered while paused
  evicted: 0, // events the server's ring dropped before we read them: gone
  cut: 0, // events a frame did not carry because of its limit: still in the ring
  dropped: 0, // frames superseded before a paint
  latest: null,
  pending: { generation: 0, count: 0 },
  drift: [],
  integrity: null,
  replay: null, // last replay SSE frame
})

export const TAIL_MAX = 500
let es = null
let delay = 1000
let inbox = [] // events waiting for the next frame (plain array: not reactive)
let nextMetrics = null // the newest metrics frame waiting for the next paint (not reactive)
let raf = 0

function schedule() {
  if (raf) return // a frame is already booked; each handler counts its own supersede
  raf = requestAnimationFrame(() => {
    raf = 0
    if (inbox.length) {
      live.tail = inbox.concat(live.tail).slice(0, TAIL_MAX)
      inbox = []
    }
    if (nextMetrics) {
      live.metrics = nextMetrics
      nextMetrics = null
    }
  })
}

// The frame budget: the animation-frame gaps of the last 3 s. `missed` is true while 3 or
// more gaps over 50 ms fall in one 3 s window and false again 10 s after the last one, so a
// machine that cannot paint says so once and Flow stops animating instead of stuttering.
export const budget = $state({ missed: false })
let overs = [] // timestamps of the gaps over 50 ms in the window
let lastOver = 0
let prevFrame = 0
function watchFrames(t) {
  requestAnimationFrame(watchFrames)
  if (prevFrame) {
    // A gap over a second is the host suspending the loop (hidden window, low power), not jank.
    const d = t - prevFrame
    if (d > 50 && d < 1000) { overs.push(t); lastOver = t }
    while (overs.length && t - overs[0] > 3000) overs.shift()
    if (overs.length >= 3) { if (!budget.missed) budget.missed = true }
    else if (budget.missed && t - lastOver > 10000) budget.missed = false
  }
  prevFrame = t
}
requestAnimationFrame(watchFrames)

export function resume() {
  live.paused = false
  live.held = 0
  inbox = []
}

export function connect() {
  if (es) es.close()
  es = new EventSource(`/api/stream?tail=${TAIL_MAX}`)
  es.onopen = () => {
    live.conn = 'live'
    live.retryIn = 0
    delay = 1000
  }
  es.onerror = () => {
    es.close()
    es = null
    live.conn = 'reconnecting'
    live.retryIn = Math.round(delay / 1000)
    setTimeout(connect, delay)
    delay = Math.min(delay * 2, 30000)
  }
  const on = (name, fn) => es.addEventListener(name, (e) => fn(JSON.parse(e.data)))
  on('hello', (h) => {
    live.latest = h.latest_raw_id
    live.pending = { generation: h.pending_generation, count: h.pending_count ?? live.pending.count }
    live.tail = (h.tail?.events ?? []).slice(-TAIL_MAX).reverse().map(row)
    live.cut = h.tail?.cut ?? 0
    live.evicted = Math.max(0, (h.tail?.skipped ?? 0) - live.cut)
    // hello may carry no count on an older server; take it from the list once.
    if (h.pending_count == null) {
      fetch('/api/pending').then((r) => r.json()).then((l) => { if (Array.isArray(l)) live.pending.count = l.length }).catch(() => {})
    }
  })
  // Only the metrics snapshot waits for the paint: a burst coalesces and the superseded ones
  // count. Drift and integrity apply at once, or a deferred older frame would undo a newer event.
  on('metrics', (m) => {
    if (m.drift) live.drift = m.drift
    if (m.integrity) live.integrity = m.integrity
    if (nextMetrics) live.dropped++
    nextMetrics = m
    schedule()
  })
  on('tail', (t) => {
    // `skipped` is the total; `cut` is the part the frame's limit left in the ring.
    live.cut += t.cut ?? 0
    live.evicted += Math.max(0, (t.skipped ?? 0) - (t.cut ?? 0))
    if (t.latest_raw_id != null) live.latest = t.latest_raw_id
    // Held: the rows on screen stay where the reader left them; new events are counted,
    // not stacked, so releasing shows the present rather than a backlog.
    if (live.paused) { live.held += t.events?.length ?? 0; return }
    if (t.events?.length) {
      if (inbox.length) live.dropped++ // the previous frame never painted; it is superseded, not queued
      inbox = t.events.slice(-TAIL_MAX).reverse().map(row).concat(inbox).slice(0, TAIL_MAX)
    }
    schedule()
  })
  on('pending', (p) => (live.pending = p))
  on('drift', (a) => {
    const i = live.drift.findIndex((x) => x.source === a.source)
    live.drift = i < 0 ? [a, ...live.drift] : live.drift.map((x, n) => (n === i ? a : x))
  })
  on('integrity', (i) => (live.integrity = i))
  on('replay', (r) => (live.replay = r))
}

export async function loadStatus() {
  try {
    const r = await fetch('/api/status')
    if (r.ok) live.status = await r.json()
  } catch { /* the status bar shows what it has */ }
}

// Pivot breadcrumb: where the investigator has walked, newest last. The plain copy is what
// pushTrail reads, so calling it from an effect does not make the effect depend on itself.
export const trail = $state({ steps: [] })
let steps = []
export function pushTrail(kind, value) {
  const at = steps.findIndex((s) => s.kind === kind && s.value === value)
  steps = at >= 0 ? steps.slice(0, at + 1) : [...steps, { kind, value }].slice(-8)
  trail.steps = steps
}
