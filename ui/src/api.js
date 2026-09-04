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
    data = { error: `${res.status} ${res.statusText}`, reason: `http_${res.status}` }
  }
  return { ok: res.ok, status: res.status, data }
}

export const fmt = {
  n: (x) => (x == null ? '–' : Number(x).toLocaleString('en-US')),
  f: (x, d = 1) => (x == null ? '–' : Number(x).toLocaleString('en-US', { minimumFractionDigits: d, maximumFractionDigits: d })),
  mb: (b) => (b == null ? '–' : (b / 1048576).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })),
  pairs: (list) => (Array.isArray(list) && list.length ? list.map(([r, n]) => `${r} ${fmt.n(n)}`).join(', ') : 'none'),
  time: (ms) => (ms == null ? '–' : new Date(ms).toISOString().replace('T', ' ').replace('Z', '')),
  cut: (s, n = 120) => (s == null ? '' : String(s).length > n ? String(s).slice(0, n - 1) + '…' : String(s)),
  json: (o) => JSON.stringify(o, null, 2),
}
