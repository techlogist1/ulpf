import { fmt, leaf, summarize } from './api.js'

// A tail row is flattened the moment it arrives: seven strings, never the nested event.
// Keeping the whole normalized object in reactive state would proxy every nested field of
// every row on every frame, which is what locks a browser at full rate.
export function row(ev) {
  const l = ev.line
  return {
    raw_id: ev.raw_id,
    time: leaf(l, 'metadata.event_time_rfc3339') ?? fmt.time(l?.time),
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
  skipped: 0, // events the server's ring evicted before we read them
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
let raf = 0

function schedule() {
  if (raf) {
    live.dropped++ // the previous frame never painted; it is superseded, not queued
    return
  }
  raf = requestAnimationFrame(() => {
    raf = 0
    if (!inbox.length) return
    live.tail = inbox.concat(live.tail).slice(0, TAIL_MAX)
    inbox = []
  })
}

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
    live.skipped = h.tail?.skipped ?? 0
    // hello may carry no count on an older server; take it from the list once.
    if (h.pending_count == null) {
      fetch('/api/pending').then((r) => r.json()).then((l) => { if (Array.isArray(l)) live.pending.count = l.length }).catch(() => {})
    }
  })
  on('metrics', (m) => {
    live.metrics = m
    if (m.drift) live.drift = m.drift
    if (m.integrity) live.integrity = m.integrity
  })
  on('tail', (t) => {
    live.skipped += t.skipped ?? 0
    if (t.latest_raw_id != null) live.latest = t.latest_raw_id
    // Held: the rows on screen stay where the reader left them; new events are counted,
    // not stacked, so releasing shows the present rather than a backlog.
    if (live.paused) { live.held += t.events?.length ?? 0; return }
    if (t.events?.length) inbox = t.events.slice(-TAIL_MAX).reverse().map(row).concat(inbox).slice(0, TAIL_MAX)
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
