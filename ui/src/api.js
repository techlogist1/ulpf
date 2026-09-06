// Every call returns { ok, status, data }. Errors from the server are
// { error, reason }; a non-JSON body becomes one with reason "http_<status>".
export async function api(method, url, body) {
  let res
  try {
    res = await fetch(url, {
      method,
      headers: body === undefined ? {} : { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    })
  } catch (e) {
    return { ok: false, status: 0, data: { error: String(e), reason: 'network' } }
  }
  let data
  try {
    data = await res.json()
  } catch {
    // A route this build does not serve answers with a bare 404 and no JSON body.
    data = {
      error: res.status === 404 ? `${String(url).split('?')[0]} is not served by this build` : `${res.status} ${res.statusText}`,
      reason: `http_${res.status}`,
    }
  }
  return { ok: res.ok, status: res.status, data }
}

export const leaf = (o, path) => String(path).split('.').reduce((a, k) => (a == null ? a : a[k]), o)

// Nested normalized object -> [[dotted path, scalar]], in schema order.
export function flat(o, prefix = '', out = []) {
  for (const [k, v] of Object.entries(o ?? {})) {
    const p = prefix ? `${prefix}.${k}` : k
    if (v && typeof v === 'object' && !Array.isArray(v)) flat(v, p, out)
    else out.push([p, Array.isArray(v) ? v.join(', ') : v])
  }
  return out
}

export const fmt = {
  n: (x) => (x == null ? '–' : Number(x).toLocaleString('en-US')),
  f: (x, d = 1) => (x == null ? '–' : Number(x).toLocaleString('en-US', { minimumFractionDigits: d, maximumFractionDigits: d })),
  mb: (b) => (b == null ? '–' : (b / 1048576).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })),
  // A difference of two rates can be -1e-17; toFixed keeps the sign as "-0.0".
  pct: (x) => (x == null ? '–' : `${(Number(x) * 100).toFixed(1).replace(/^-(0(\.0+)?)$/, '$1')}%`),
  pairs: (list) => (Array.isArray(list) && list.length ? list.map(([r, n]) => `${r} ${fmt.n(n)}`).join('  ') : 'none'),
  time: (ms) => (ms == null ? '–' : new Date(ms).toISOString().replace('T', ' ').replace('Z', '')),
  // "2026-09-04T10:23:00.000Z" or epoch ms -> "09-04 10:23:00.000Z". The year is identical on
  // every row of a tail, and carrying it truncated the seconds, which is the part being read.
  // The zone is kept: without it a UTC stamp reads as local time five hours in the past.
  stamp: (v) => {
    if (v == null || v === '') return '–'
    const s = typeof v === 'number' ? `${fmt.time(v)}Z` : String(v)
    const m = s.match(/\d{4}-(\d{2}-\d{2})[T ](\d{2}:\d{2}:\d{2}(?:\.\d{1,3})?)\d*(Z|[+-]\d{2}:?\d{2})?/)
    return m ? `${m[1]} ${m[2]}${m[3] ?? ''}` : s
  },
  clock: (ms) => (ms == null ? '–' : new Date(ms).toISOString().slice(11, 23)),
  day: (ms) => (ms == null ? '–' : new Date(ms).toISOString().slice(0, 10)),
  ago: (s) => {
    if (s == null) return '–'
    const n = Math.floor(s)
    return n < 60 ? `${n}s` : n < 3600 ? `${Math.floor(n / 60)}m ${n % 60}s` : `${Math.floor(n / 3600)}h ${Math.floor((n % 3600) / 60)}m`
  },
  cut: (s, n = 160) => (s == null ? '' : String(s).length > n ? String(s).slice(0, n - 1) + '…' : String(s)),
  hex: (h) => (h == null ? '–' : `${String(h).slice(0, 8)}…${String(h).slice(-8)}`),
  json: (o) => JSON.stringify(o, null, 2),
}

// One line of the fields an analyst reads first, in the order they read them.
export function summarize(line) {
  const p = (k) => leaf(line, k)
  const ep = (a, b) => (p(a) == null ? null : p(b) == null ? String(p(a)) : `${p(a)}:${p(b)}`)
  const out = []
  const src = ep('src_endpoint.ip', 'src_endpoint.port')
  const dst = ep('dst_endpoint.ip', 'dst_endpoint.port')
  // src > dst when both are known; a lone endpoint stands on its own rather than
  // beside a placeholder for the half the event does not carry.
  if (src && dst) out.push(`${src} > ${dst}`)
  else if (src) out.push(src)
  else if (dst) out.push(`> ${dst}`)
  for (const k of ['connection_info.protocol_name', 'app_name', 'actor.user.name', 'user.name', 'firewall_rule.name', 'finding_info.title', 'http_request.url.hostname', 'dns_query.hostname']) {
    const v = p(k)
    if (v != null && v !== '') out.push(String(v))
  }
  if (!out.length && line?.message) out.push(String(line.message))
  return out.join('  ')
}
