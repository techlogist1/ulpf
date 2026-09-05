<script>
  import { api, fmt, flat } from './api.js'
  import { keys, nav } from './keys.js'
  import VList from './VList.svelte'

  let { id = '' } = $props()
  let input = $state('')
  let data = $state(null)
  let err = $state(null)
  let busy = $state(false)
  // What is lit is one byte range, not one field name: two fields can carry the same key.
  let over = $state(null) // { id, key } under the pointer or focus
  let pin = $state(null)  // click locks a range lit so both sides can be read at once
  const idOf = (span) => (span ? `${span[0]}:${span[1]}` : null)
  let sel = $state(-1)
  let showHex = $state(false)
  let box = $state(null)
  let wrapEl = $state(null)
  let width = $state(1200)

  async function load(rid) {
    if (rid === '' || rid == null) return
    busy = true; err = null; data = null; over = null; pin = null; sel = -1
    const r = await api('GET', `/api/events/${encodeURIComponent(rid)}`)
    busy = false
    if (r.ok) data = r.data
    else err = r.data
  }
  $effect(() => { input = id; load(id) })
  $effect(() => {
    const measure = () => { if (wrapEl) width = wrapEl.clientWidth }
    measure()
    window.addEventListener('resize', measure)
    return () => window.removeEventListener('resize', measure)
  })
  $effect(() => { if (wrapEl && data) width = wrapEl.clientWidth })

  function go(e) {
    e.preventDefault()
    const v = input.trim()
    if (v !== '') location.hash = `#/trace/${encodeURIComponent(v)}`
  }

  const hot = $derived(pin ?? over)
  const bytes = $derived.by(() => {
    const h = data?.hex ?? ''
    const a = new Uint8Array(h.length / 2)
    for (let i = 0; i < a.length; i++) a[i] = parseInt(h.slice(i * 2, i * 2 + 2), 16)
    return a
  })
  const fields = $derived(data?.now?.fields ?? null)
  const prov = $derived(data?.now?.provenance ?? null)
  const timeSpan = $derived(data?.now?.time?.text_span ?? null)

  // One tint per source key, assigned in the order the parser produced them.
  const colourOf = $derived.by(() => {
    const map = new Map()
    for (const f of fields ?? []) if (!map.has(f.key)) map.set(f.key, `var(--p${map.size % 8})`)
    for (const p of prov ?? []) if (!map.has(p.source_key)) map.set(p.source_key, `var(--p${map.size % 8})`)
    return map
  })
  const tint = (k) => colourOf.get(k) ?? 'var(--line-3)'

  // Byte range -> owning key, non-overlapping, in byte order. The timestamp is added last
  // and dropped where a parser field already owns those bytes.
  const spans = $derived.by(() => {
    const raw = []
    for (const f of fields ?? []) if (f.span) raw.push({ key: f.key, s: f.span[0], e: f.span[1], time: false })
    if (!fields) for (const p of prov ?? []) if (p.span) raw.push({ key: p.source_key, s: p.span[0], e: p.span[1], time: false })
    // A field that strictly contains another reported field is an envelope (`body` around the
    // key/value pairs parsed out of it). Light the parts, not the wrapper.
    // ponytail: O(n²) over one record's fields; a sweep if a parser ever reports thousands.
    const inner = raw.filter((a, i) => !raw.some((b, j) => j !== i && b.s >= a.s && b.e <= a.e && b.e - b.s < a.e - a.s))
    inner.sort((a, b) => a.s - b.s || a.e - b.e)
    const out = []
    let clipped = 0
    for (const r of inner) {
      if (r.e > r.s && (!out.length || r.s >= out[out.length - 1].e)) out.push(r)
      else clipped++
    }
    if (timeSpan) {
      const [s, e] = timeSpan
      if (!out.some((o) => s < o.e && e > o.s)) { out.push({ key: '(timestamp)', s, e, time: true }); out.sort((a, b) => a.s - b.s) }
    }
    for (const o of out) o.id = `${o.s}:${o.e}`
    return { list: out, overlapped: clipped }
  })
  const owned = $derived(spans.list)
  const overlapped = $derived(spans.overlapped)

  // The byte ruler: fixed-width rows so a 4 MB record is 30,000 rows of which 30 are in the
  // DOM. Text rows never split a UTF-8 sequence; hex rows are the classic sixteen.
  const cols = $derived(showHex ? 16 : Math.max(32, Math.min(256, Math.floor((width - 96) / 7.2))))
  const starts = $derived.by(() => {
    const n = bytes.length
    const out = []
    let at = 0
    while (at < n) {
      out.push(at)
      let next = at + cols
      if (!showHex) while (next < n && (bytes[next] & 0xc0) === 0x80) next--
      if (next <= at) next = at + cols
      at = next
    }
    if (!out.length) out.push(0)
    return out
  })
  const decoder = new TextDecoder('utf-8', { fatal: false })
  // Control bytes are shown as \xNN so nothing in the record is invisible.
  function text(from, to) {
    const t = decoder.decode(bytes.subarray(from, to))
    const out = []
    let run = ''
    for (const ch of t) {
      const c = ch.codePointAt(0)
      if (c < 0x20 || c === 0x7f) { if (run) { out.push({ t: run }); run = '' } out.push({ t: '\\x' + c.toString(16).padStart(2, '0'), ctl: true }) }
      else run += ch
    }
    if (run) out.push({ t: run })
    return out
  }
  // Segments of one row: plain runs and owned runs, from the sorted span list.
  function segments(s, e) {
    const out = []
    let lo = 0, hi = owned.length
    while (lo < hi) { const m = (lo + hi) >> 1; if (owned[m].e <= s) lo = m + 1; else hi = m }
    let at = s
    for (let i = lo; i < owned.length && owned[i].s < e; i++) {
      const o = owned[i]
      const a = Math.max(o.s, s), b = Math.min(o.e, e)
      if (a > at) out.push({ s: at, e: a })
      out.push({ s: a, e: b, o })
      at = b
    }
    if (at < e) out.push({ s: at, e })
    return out
  }
  const rowEnd = (i) => (i + 1 < starts.length ? starts[i + 1] : bytes.length)
  const hex2 = (b) => b.toString(16).padStart(2, '0')
  const asc = (b) => (b >= 32 && b < 127 ? String.fromCharCode(b) : '.')
  const rowHot = (i) => hot && hot.s < rowEnd(i) && hot.e > starts[i]
  const rangeOf = (o) => ({ id: o.id, key: o.key, s: o.s, e: o.e })
  const toggle = (o) => (pin = pin?.id === o.id ? null : rangeOf(o))
  const byId = (sid) => owned.find((o) => o.id === sid)
  const lightRow = (r) => { const o = r.span && byId(idOf(r.span)); over = o ? rangeOf(o) : null }
  const pinRow = (r) => { const o = r.span && byId(idOf(r.span)); if (o) toggle(o) }

  const provRows = $derived(prov ?? [])
  // Three different reasons nothing is lit, and they mean different things at 3am.
  const spanNote = $derived(
    owned.length
      ? `${owned.length} lit range${owned.length === 1 ? '' : 's'}: hover either side, click to keep one lit`
      : !fields && !prov
        ? 'this server reports no field spans'
        : fields?.length
          ? 'every value was materialised (a JSON value, an unescaped string, a joined timestamp): no range is a slice of these bytes'
          : 'no parser claimed this record, so nothing points into its bytes',
  )
  $effect(() => keys((ev) => {
    if (ev.key === '/') { box?.focus(); box?.select(); return true }
    if (ev.key === 'Escape' && pin) { pin = null; return true }
    if (ev.key === 'h') { showHex = !showHex; return true }
    return nav(ev, provRows.length, sel, (n) => { sel = n; lightRow(provRows[n]) }, (n) => pinRow(provRows[n]))
  }))

  const diffPaths = $derived.by(() => {
    if (!data?.emitted || !data?.now?.normalized) return null
    const skip = (p) => p.startsWith('metadata.processed_time')
    const a = new Map(flat(data.emitted).filter(([p]) => !skip(p)))
    const b = new Map(flat(data.now.normalized).filter(([p]) => !skip(p)))
    const out = []
    for (const [p, v] of a) if (!b.has(p)) out.push([p, String(v), '–'])
    for (const [p, v] of b) if (!a.has(p)) out.push([p, '–', String(v)])
      else if (String(a.get(p)) !== String(v)) out.push([p, String(a.get(p)), String(v)])
    return out
  })
  const timeText = $derived(timeSpan ? decoder.decode(bytes.subarray(timeSpan[0], timeSpan[1])) : null)
  const policies = $derived(data?.now?.time?.policies ?? data?.emitted?.ulpf?.time_policies ?? [])
</script>

<section>
  <div class="head">
    <h2>Traceback</h2>
    <span class="note">one emitted line back to the bytes it came from</span>
    <form class="bar push" onsubmit={go}>
      <label class="sm muted" for="rid">raw id</label>
      <input id="rid" type="search" inputmode="numeric" bind:value={input} bind:this={box} placeholder="raw id  /" size="12" />
      <button class="btn primary" type="submit" disabled={busy}>Look up</button>
    </form>
  </div>
  {#if busy}<p class="loading">reading record {id} through the writer's lock</p>{/if}
</section>

{#if err}
  <div class="notice bad">
    <b>{err.error}</b>
    <span class="muted">{err.reason}{#if err.status != null && err.status !== 404}, HTTP {err.status}{/if}</span>
    {#if err.store_len != null}<span>The store holds {fmt.n(err.store_len)} records, ids 0 to {fmt.n(Math.max(0, err.store_len - 1))}. Open one from Live, or enter an id in that range.</span>{/if}
  </div>
{:else if !data && !id}
  <div class="empty">
    <b>No record chosen.</b>
    <span>Enter a raw id above, press Enter on a row in Live, or follow an event from Pivot or Replay.</span>
    <span class="sm">What you get: the exact stored bytes with every parsed field's range lit, the digest re-checked now, and the record's place in the hash chain.</span>
  </div>
{:else if data}
  <section class="stack">
    <div class="facts">
      <div><span>raw id</span><b>{data.raw_id}</b></div>
      <div><span>source</span><b>{data.source}</b></div>
      <div><span>receipt</span><b>{data.receipt}</b></div>
      <div><span>bytes</span><b>{fmt.n(data.bytes_len)}</b></div>
      <div><span>parser now</span><b class:is-warn={!data.now?.parser}>{data.now?.parser ?? 'none'}</b></div>
      <div><span>status</span><b class:is-warn={data.now?.parse_status !== 'parsed'}>{data.now?.parse_status}</b></div>
    </div>

    <div class="verdicts">
      <div class="verdict" class:ok={data.digest_match} class:bad={!data.digest_match}>
        <b>{data.digest_match ? 'Bytes unchanged since receipt' : 'Bytes do not match their digest'}</b>
        <span class="lab">stored SHA-256</span><pre>{data.stored_sha256}</pre>
        <span class="lab">recomputed now</span><pre>{data.recomputed_sha256}</pre>
      </div>
      {#if data.chain}
        <div class="verdict" class:ok={data.chain_match} class:bad={!data.chain_match}>
          <b>{data.chain_match ? 'Follows the record before it' : 'Chain value does not follow from the previous record'}</b>
          <span class="lab">prev_chain</span><pre>{data.prev_chain}</pre>
          <span class="lab">chain = sha256(prev_chain ‖ digest)</span><pre>{data.chain}</pre>
        </div>
      {/if}
      <div class="verdict" class:ok={timeSpan && !policies.length} class:warn={!timeSpan || policies.length}>
        <b>{timeSpan ? (policies.length ? 'Device time read, with a policy applied' : 'Device time read from the bytes') : 'Device time not found: receipt time used'}</b>
        {#if timeSpan}
          <span class="lab">bytes {timeSpan[0]}–{timeSpan[1]}</span><pre>{timeText}</pre>
        {:else if data.emitted?.metadata?.original_time}
          <span class="lab">original_time as emitted</span><pre>{data.emitted.metadata.original_time}</pre>
        {/if}
        <div class="bar">{#each policies as p}<span class="tag warn">{p}</span>{/each}</div>
      </div>
    </div>

    <div>
      <div class="head">
        <h2>Raw record</h2>
        <span class="note">{spanNote}</span>
        {#if overlapped > 0}<span class="tag warn" title="two reported ranges cover the same bytes; the narrower one is shown">{overlapped} overlapping not lit</span>{/if}
        <span class="push bar">
          {#if pin}<span class="pinned" style="--c:{tint(pin.key)}"><b>{pin.key}</b> {pin.s}–{pin.e} held, Esc releases</span>{/if}
          <button class="btn" class:on={showHex} onclick={() => (showHex = !showHex)}>{showHex ? 'Text' : 'Hex'}<kbd>h</kbd></button>
        </span>
      </div>
      {#if colourOf.size}
        <div class="legend" style="margin-bottom:var(--s3)">
          {#each [...colourOf] as [k, c] (k)}<span style="--c:{c}"><i class="sw"></i>{k}</span>{/each}
          {#if timeSpan}<span style="--c:var(--fg-2)"><i class="sw" style="background:none;box-shadow:inset 0 -2px 0 var(--fg-2)"></i>timestamp</span>{/if}
        </div>
      {/if}
      <div class="bytes" class:hexmode={showHex} bind:this={wrapEl} style="--cols:7ch minmax(0,1fr){showHex ? ' 16ch' : ''}">
        <VList items={starts} max={showHex ? 528 : 396} rowH={22}>
          {#snippet header()}
            <div class="vh"><span class="off">offset</span><span>{showHex ? 'bytes, sixteen per row' : `${cols} bytes per row`}</span>{#if showHex}<span>ascii</span>{/if}</div>
          {/snippet}
          {#snippet row(s, i)}
            {@const e = rowEnd(i)}
            <div class="vr static" class:mark={rowHot(i)}>
              <span class="off">{showHex ? s.toString(16).padStart(6, '0') : s}</span>
              {#if showHex}
                <span class="hex">{#each segments(s, e) as g}{#if g.o}<span class="sp" class:time={g.o.time} class:hot={hot?.id === g.o.id} style="--c:{g.o.time ? 'var(--fg-2)' : tint(g.o.key)}" title="{g.o.key}  bytes {g.o.s}–{g.o.e}" onmouseenter={() => (over = rangeOf(g.o))} onmouseleave={() => (over = null)} onclick={() => toggle(g.o)} role="button" tabindex="-1">{[...bytes.subarray(g.s, g.e)].map(hex2).join(' ')}</span>{:else}{[...bytes.subarray(g.s, g.e)].map(hex2).join(' ')}{/if}{#if g.e < e}{' '}{/if}{/each}</span>
                <span class="asc">{#each segments(s, e) as g}{#if g.o}<span class="sp" class:hot={hot?.id === g.o.id} style="--c:{g.o.time ? 'var(--fg-2)' : tint(g.o.key)}">{[...bytes.subarray(g.s, g.e)].map(asc).join('')}</span>{:else}{[...bytes.subarray(g.s, g.e)].map(asc).join('')}{/if}{/each}</span>
              {:else}
                <span class="txt">{#each segments(s, e) as g}{#if g.o}<span class="sp" class:time={g.o.time} class:hot={hot?.id === g.o.id} style="--c:{g.o.time ? 'var(--fg-2)' : tint(g.o.key)}" title="{g.o.key}  bytes {g.o.s}–{g.o.e}  (click to keep it lit)" onmouseenter={() => (over = rangeOf(g.o))} onmouseleave={() => (over = null)} onclick={() => toggle(g.o)} role="button" tabindex="-1">{#each text(g.s, g.e) as p}{#if p.ctl}<i class="ctl">{p.t}</i>{:else}{p.t}{/if}{/each}</span>{:else}{#each text(g.s, g.e) as p}{#if p.ctl}<i class="ctl">{p.t}</i>{:else}{p.t}{/if}{/each}{/if}{/each}</span>
              {/if}
            </div>
          {/snippet}
        </VList>
      </div>
      {#if !fields && !prov}
        <p class="notice sm" style="margin-top:var(--s3)">This server answers the v1 contract: bytes and parsed result, no per-field byte spans, so nothing is lit above.</p>
      {/if}
    </div>

    <div class="split">
      <div class="prov" style="--cols:minmax(0,1.1fr) minmax(0,1.6fr) 9em">
        <div class="head"><h2>Parser fields</h2><span class="note">the device's own vocabulary, in parser order, {fmt.n(fields?.length ?? 0)} pairs</span></div>
        {#if fields?.length}
          <VList items={fields} max={330}>
            {#snippet header()}<div class="vh"><span>key</span><span>value</span><span class="num">bytes</span></div>{/snippet}
            {#snippet row(f)}
              {@const fid = idOf(f.span)}
              <div class="vr" class:hot={fid && hot?.id === fid} class:pin={fid && pin?.id === fid} style="--c:{tint(f.key)}"
                   onmouseenter={() => lightRow(f)} onmouseleave={() => (over = null)} onclick={() => pinRow(f)} role="button" tabindex="-1">
                <span class="k"><i class="sw"></i>{f.key}</span>
                <span class="v" title={f.value}>{f.value}</span>
                <span class="num from">{#if f.span}{f.span[0]}–{f.span[1]}{:else}derived{/if}</span>
              </div>
            {/snippet}
          </VList>
        {:else if fields}
          <div class="empty"><b>No fields.</b><span>The parser claimed this record but produced no key/value pairs.</span></div>
        {:else}
          <div class="empty"><b>No parser fields reported.</b><span>The normalized result is below.</span></div>
        {/if}
      </div>
      <div class="prov" style="--cols:minmax(0,1.2fr) minmax(0,1.4fr) 10em">
        <div class="head"><h2>Normalized</h2><span class="note">schema path, and the field it came from</span></div>
        {#if provRows.length}
          <VList items={provRows} max={330} {sel}>
            {#snippet header()}<div class="vh"><span>path</span><span>value</span><span>from</span></div>{/snippet}
            {#snippet row(p, i)}
              {@const pid = idOf(p.span)}
              <div class="vr" class:hot={pid && hot?.id === pid} class:pin={pid && pin?.id === pid} class:sel={sel === i} style="--c:{tint(p.source_key)}"
                   onmouseenter={() => lightRow(p)} onmouseleave={() => (over = null)} onclick={() => { sel = i; pinRow(p) }} role="button" tabindex="-1">
                <span class="k"><i class="sw"></i>{p.path}</span>
                <span class="v" title={p.value}>{p.value}{#if p.canonical} <span class="tag" title="the mapping rewrote this value">canonical</span>{/if}</span>
                <span class="from">{p.source_key}{#if !p.span} · derived{/if}</span>
              </div>
            {/snippet}
          </VList>
          <p class="xs muted" style="margin-top:var(--s2)">Fields the mapping synthesised (class_uid, metadata) have no source field and are not listed.</p>
        {:else}
          <div class="empty"><b>Nothing normalized from a source field.</b><span>{data.now?.parser ? 'The mapping produced only synthesised fields for this parser.' : 'No parser claimed this record.'}</span></div>
        {/if}
      </div>
    </div>

    <div class="split">
      <div>
        <div class="head"><h2>Emitted</h2><span class="note">as written to the output</span></div>
        {#if data.emitted}
          <pre class="json">{fmt.json(data.emitted)}</pre>
        {:else}
          <div class="empty"><b>Not in the tail any more.</b><span>The output file holds the emitted line; the result of parsing the bytes now is on the right.</span></div>
        {/if}
      </div>
      <div>
        <div class="head"><h2>Now</h2><span class="note">the same bytes through the parsers loaded right now</span></div>
        <pre class="json">{fmt.json(data.now?.normalized ?? {})}</pre>
        {#if diffPaths}
          {#if diffPaths.length}
            <table class="tbl" style="margin-top:var(--s3)">
              <thead><tr><th>changed path</th><th>emitted</th><th>now</th></tr></thead>
              <tbody>{#each diffPaths as [p, a, b]}<tr><td class="mono">{p}</td><td class="mono is-dim">{fmt.cut(a, 40)}</td><td class="mono is-warn">{fmt.cut(b, 40)}</td></tr>{/each}</tbody>
            </table>
          {:else}
            <p class="sm is-ok" style="margin-top:var(--s3)">Identical: the parsers loaded now produce exactly what was emitted.</p>
          {/if}
        {/if}
      </div>
    </div>
  </section>
{/if}
