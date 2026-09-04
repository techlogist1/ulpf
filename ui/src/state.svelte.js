// Live state fed by GET /api/stream (events: hello, metrics, tail, pending).
export const live = $state({
  conn: 'connecting', // connecting | live | reconnecting
  retryIn: 0,
  metrics: null,
  tail: [],
  skipped: 0,
  latest: null,
  pending: { generation: 0, count: 0 },
})

const TAIL_MAX = 500
let es = null
let delay = 1000

export function connect() {
  if (es) es.close()
  es = new EventSource('/api/stream?tail=100')
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
  es.addEventListener('hello', (e) => {
    const h = JSON.parse(e.data)
    live.latest = h.latest_raw_id
    live.pending = { generation: h.pending_generation, count: live.pending.count }
    live.tail = (h.tail?.events ?? []).slice().reverse()
    live.skipped = h.tail?.skipped ?? 0
    // hello carries no pending count; take it from the list once so the nav badge is right before the first change.
    fetch('/api/pending').then((r) => r.json()).then((l) => { if (Array.isArray(l)) live.pending.count = l.length }).catch(() => {})
  })
  es.addEventListener('metrics', (e) => {
    live.metrics = JSON.parse(e.data)
  })
  es.addEventListener('tail', (e) => {
    const t = JSON.parse(e.data)
    if (t.events?.length) live.tail = t.events.slice().reverse().concat(live.tail).slice(0, TAIL_MAX)
    live.skipped += t.skipped ?? 0
    if (t.latest_raw_id != null) live.latest = t.latest_raw_id
  })
  es.addEventListener('pending', (e) => {
    live.pending = JSON.parse(e.data)
  })
}
