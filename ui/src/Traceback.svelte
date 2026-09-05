<script>
  import { api, fmt, flat } from './api.js'
  import { keys, nav } from './keys.js'

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

  async function load(rid) {
    if (rid === '' || rid == null) return
    busy = true; err = null; data = null; over = null; pin = null; sel = -1
    const r = await api('GET', `/api/events/${encodeURIComponent(rid)}`)
    busy = false
    if (r.ok) data = r.data
    else err = r.data
  }
  $effect(() => { input = id; load(id) })

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

  // One colour per source key, assigned in the order the parser produced them.
  const colourOf = $derived.by(() => {
    const map = new Map()
    for (const f of fields ?? []) if (!map.has(f.key)) map.set(f.key, `var(--p${map.size % 8})`)
    for (const p of prov ?? []) if (!map.has(p.source_key)) map.set(p.source_key, `var(--p${map.size % 8})`)
    return map
  })

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
    // Shortest first at any start, then greedy: whatever still overlaps is counted, not hidden.
    inner.sort((a, b) => a.s - b.s || a.e - b.e)
    const out = []
    let clipped = 0
    for (const r of inner) {
      if (r.e > r.s && (!out.length || r.s >= out[out.length - 1].e)) out.push(r)
      else clipped++
    }
    if (timeSpan) {
      const [s, e] = timeSpan
      const clash = out.some((o) => s < o.e && e > o.s)
      if (!clash) {
        out.push({ key: '(timestamp)', s, e, time: true })
        out.sort((a, b) => a.s - b.s)
      }
    }
    return { list: out, overlapped: clipped }
  })
  const owned = $derived(spans.list)
  const overlapped = $derived(spans.overlapped)

  const decoder = new TextDecoder('utf-8', { fatal: false })
  // Pieces of one chunk, control bytes shown as \xNN so nothing in the record is invisible.
  function pieces(from, to) {
    const text = decoder.decode(bytes.slice(from, to))
    const out = []
    let run = ''
    for (const ch of text) {
      const c = ch.codePointAt(0)
      if (c < 0x20 || c === 0x7f) { if (run) { out.push({ t: run, ctl: false }); run = '' } out.push({ t: '\\x' + c.toString(16).padStart(2, '0'), ctl: true }) }
      else run += ch
    }
    if (run) out.push({ t: run, ctl: false })
    return out
  }

  const chunks = $derived.by(() => {
    const out = []
    let at = 0
    for (const o of owned) {
      if (o.s > at) out.push({ key: null, pieces: pieces(at, o.s) })
      out.push({ key: o.key, id: `${o.s}:${o.e}`, time: o.time, pieces: pieces(o.s, o.e), span: [o.s, o.e] })
      at = o.e
    }
    if (at < bytes.length) out.push({ key: null, pieces: pieces(at, bytes.length) })
    return out
  })

  const provRows = $derived(prov ?? [])
  // Three different reasons nothing is lit, and they mean different things at 3am.
  const spanNote = $derived(
    owned.length
      ? 'each lit range is one parsed field: hover either side to light it, click to keep it lit'
      : !fields && !prov
        ? 'this server reports no field spans'
        : fields?.length
          ? 'every value in this record was materialised (a JSON value, an unescaped string, a joined timestamp), so no range is a slice of these bytes'
          : 'no parser claimed this record, so nothing points into its bytes',
  )
  $effect(() => keys((ev) => {
    if (ev.key === '/') { box?.focus(); box?.select(); return true }
    if (ev.key === 'Escape' && pin) { pin = null; return true }
    if (ev.key === 'h') { showHex = !showHex; return true }
    return nav(ev, provRows.length, sel, (n) => {
      sel = n
      const r = provRows[n]
      over = r?.span ? { id: idOf(r.span), key: r.source_key } : null
    }, (n) => {
      const r = provRows[n]
      const id = idOf(r?.span)
      if (id) pin = pin?.id === id ? null : { id, key: r.source_key }
    })
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

  const dump = (a) => {
    const rows = []
    for (let i = 0; i < a.length; i += 16) {
      const s = [...a.slice(i, i + 16)]
      rows.push(`${i.toString(16).padStart(8, '0')}  ${s.map((b) => b.toString(16).padStart(2, '0')).join(' ').padEnd(47)}  ${s.map((b) => (b >= 32 && b < 127 ? String.fromCharCode(b) : '.')).join('')}`)
    }
    return rows.join('\n')
  }
</script>

<section>
  <div class="head">
    <h2>Traceback</h2>
    <span class="note">every emitted line back to the bytes it came from</span>
    <form class="bar push" onsubmit={go}>
      <label class="sm muted" for="rid">raw id</label>
      <input id="rid" type="search" inputmode="numeric" bind:value={input} bind:this={box} placeholder="4211" size="10" />
      <button class="btn primary" type="submit" disabled={busy}>Look up</button>
      {#if busy}<span class="muted sm">loading</span>{/if}
    </form>
  </div>
</section>

{#if err}
  <div class="notice bad">
    <b>{err.error}</b> <span class="muted">({err.reason})</span>
    {#if err.store_len != null}<p class="sm">The store holds {fmt.n(err.store_len)} records; ids run 0 to {fmt.n(Math.max(0, err.store_len - 1))}.</p>{/if}
  </div>
{:else if !data && !id}
  <p class="empty">Enter a raw id, or open a row from Live. The record's exact stored bytes are shown with every parsed field's span lit, its digest re-checked now, and its place in the hash chain.</p>
{:else if data}
  <section class="stack">
    <div class="facts">
      <div><span>raw_id</span><b>{data.raw_id}</b></div>
      <div><span>source</span><b>{data.source}</b></div>
      <div><span>receipt</span><b>{data.receipt}</b></div>
      <div><span>bytes</span><b>{fmt.n(data.bytes_len)}</b></div>
      <div><span>parser now</span><b>{data.now?.parser ?? 'none'}</b></div>
      <div><span>status</span><b class:is-warn={data.now?.parse_status !== 'parsed'}>{data.now?.parse_status}</b></div>
    </div>

    <div class="chain">
      <div class="d">
        <span class="lab">stored SHA-256</span><pre>{data.stored_sha256}</pre>
        <span class="lab">recomputed now</span><pre>{data.recomputed_sha256}</pre>
        <p class="sm" class:is-ok={data.digest_match} class:is-bad={!data.digest_match}>
          {data.digest_match ? 'The bytes still hash to the digest recorded when they arrived.' : 'The bytes do not hash to the recorded digest.'}
        </p>
      </div>
      {#if data.chain}
        <div class="d">
          <span class="lab">prev_chain</span><pre>{data.prev_chain}</pre>
          <span class="lab">chain</span><pre>{data.chain}</pre>
          <p class="sm" class:is-ok={data.chain_match} class:is-bad={!data.chain_match}>
            {data.chain_match ? 'sha256(prev_chain ‖ digest) equals chain: this record follows the one before it.' : 'The chain value does not follow from its predecessor.'}
          </p>
        </div>
      {/if}
      <div class="d">
        <span class="lab">timestamp</span>
        {#if data.now?.time}
          <pre>{data.now.time.text_span ? decoder.decode(bytes.slice(data.now.time.text_span[0], data.now.time.text_span[1])) : 'not found in the bytes'}</pre>
          <p class="sm muted">
            {#if data.now.time.text_span}bytes {data.now.time.text_span[0]}–{data.now.time.text_span[1]}{:else}taken from the receipt time{/if}
          </p>
          <div class="bar">{#each data.now.time.policies ?? [] as p}<span class="tag warn">{p}</span>{:else}<span class="tag ok">no policy applied</span>{/each}</div>
        {:else}
          <pre class="muted">{data.emitted?.metadata?.original_time ?? '–'}</pre>
          <div class="bar">{#each data.emitted?.ulpf?.time_policies ?? [] as p}<span class="tag warn">{p}</span>{/each}</div>
        {/if}
      </div>
    </div>

    <div>
      <div class="head">
        <h2>Raw record</h2>
        <span class="note">{spanNote}</span>
        {#if overlapped > 0}<span class="tag warn" title="two reported ranges cover the same bytes; the narrower one is shown">{overlapped} overlapping range{overlapped === 1 ? '' : 's'} not lit</span>{/if}
        {#if pin}<span class="tag accent">{pin.key} bytes {pin.id.replace(':', '–')} held lit, Esc releases</span>{/if}
        <span class="push"><button class="btn" onclick={() => (showHex = !showHex)}>{showHex ? 'Hide hex' : 'Show hex'}</button></span>
      </div>
      <div class="raw">{#each chunks as c}{#if c.key}<span
            class="sp"
            class:hot={hot?.id === c.id} class:pin={pin?.id === c.id}
            style="background:color-mix(in srgb, {colourOf.get(c.key) ?? 'var(--rule-strong)'} 22%, transparent); color:{colourOf.get(c.key) ?? 'var(--ink)'}"
            tabindex="0"
            role="button"
            title="{c.key}  bytes {c.span[0]}–{c.span[1]}  (click to keep it lit)"
            onmouseenter={() => (over = { id: c.id, key: c.key })}
            onmouseleave={() => (over = null)}
            onfocus={() => (over = { id: c.id, key: c.key })}
            onblur={() => (over = null)}
            onclick={() => (pin = pin?.id === c.id ? null : { id: c.id, key: c.key })}
            onkeydown={(ev) => { if (ev.key === 'Enter' || ev.key === ' ') { pin = pin?.id === c.id ? null : { id: c.id, key: c.key }; ev.preventDefault() } }}
          >{#each c.pieces as p}{#if p.ctl}<i class="ctl">{p.t}</i>{:else}{p.t}{/if}{/each}</span>{:else}{#each c.pieces as p}{#if p.ctl}<i class="ctl">{p.t}</i>{:else}{p.t}{/if}{/each}{/if}{/each}</div>
      {#if !fields && !prov}
        <p class="notice sm">This server answers the v1 contract: it returns the bytes and the parsed result, but no per-field byte spans, so nothing is lit above.</p>
      {:else if !owned.length && fields?.length}
        <p class="notice sm">Every field below is marked <b>derived</b>: this parser materialises its values rather than borrowing them from the record, so the mapping from field to bytes is the key itself, not a range.</p>
      {/if}
      {#if showHex}<pre class="raw" style="line-height:1.5">{dump(bytes)}</pre>{/if}
    </div>

    <div class="split">
      <div>
        <div class="head"><h2>Parser fields</h2><span class="note">the device's own vocabulary, in parser order</span></div>
        {#if fields}
          <div class="scroll">
            <table class="tbl prov">
              <thead><tr><th>key</th><th>value</th><th class="num">bytes</th></tr></thead>
              <tbody>
                {#each fields as f, i (f.key + i)}
                  {@const fid = idOf(f.span)}
                  <tr class="click" class:hot={fid && hot?.id === fid} class:pin={fid && pin?.id === fid}
                      onmouseenter={() => (over = fid && { id: fid, key: f.key })} onmouseleave={() => (over = null)}
                      onclick={() => (pin = !fid ? pin : pin?.id === fid ? null : { id: fid, key: f.key })}>
                    <td class="k"><i class="swatch" style="background:{colourOf.get(f.key) ?? 'var(--rule-strong)'}"></i>{f.key}</td>
                    <td class="v">{fmt.cut(f.value, 90)}</td>
                    <td class="num">{#if f.span}{f.span[0]}–{f.span[1]}{:else}<span class="tag">derived</span>{/if}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else}
          <p class="empty">This server does not report the parser's own key/value pairs. The normalized result is below.</p>
        {/if}
      </div>
      <div>
        <div class="head"><h2>Normalized</h2><span class="note">schema path, and the field it came from</span></div>
        {#if prov?.length}
          <div class="scroll">
            <table class="tbl prov">
              <thead><tr><th>path</th><th>value</th><th>from</th></tr></thead>
              <tbody>
                {#each provRows as p, i (p.path + i)}
                  {@const pid = idOf(p.span)}
                  <tr class="click" class:hot={pid && hot?.id === pid} class:pin={pid && pin?.id === pid} class:sel={sel === i}
                      tabindex="0" role="button"
                      onmouseenter={() => (over = pid && { id: pid, key: p.source_key })} onmouseleave={() => (over = null)}
                      onfocus={() => (over = pid && { id: pid, key: p.source_key })} onblur={() => (over = null)}
                      onclick={() => (pin = !pid ? pin : pin?.id === pid ? null : { id: pid, key: p.source_key })}
                      onkeydown={(ev) => { if (ev.key === 'Enter') { if (pid) pin = pin?.id === pid ? null : { id: pid, key: p.source_key }; ev.preventDefault() } }}>
                    <td class="k"><i class="swatch" style="background:{colourOf.get(p.source_key) ?? 'var(--rule-strong)'}"></i>{p.path}</td>
                    <td class="v">{fmt.cut(p.value, 60)} {#if p.canonical}<span class="tag accent" title="the mapping rewrote this value">canonical</span>{/if}</td>
                    <td class="v is-dim">{p.source_key}{#if !p.span}<span class="tag"> derived</span>{/if}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
          <p class="sm muted">Fields the mapping synthesised (class_uid, metadata) have no source field and are not listed.</p>
        {:else}
          <p class="empty">This server does not report which source field fed each schema field. The normalized object is below.</p>
        {/if}
      </div>
    </div>

    <div class="split">
      <div>
        <div class="head"><h2>Emitted</h2><span class="note">as written to the output</span></div>
        {#if data.emitted}
          <pre class="panel pad" style="max-height:40vh;overflow:auto">{fmt.json(data.emitted)}</pre>
        {:else}
          <p class="empty">Not in the tail any more. The output file holds the emitted line.</p>
        {/if}
      </div>
      <div>
        <div class="head"><h2>Now</h2><span class="note">the same bytes through the parsers as they are loaded right now</span></div>
        <pre class="panel pad" style="max-height:40vh;overflow:auto">{fmt.json(data.now?.normalized ?? {})}</pre>
        {#if diffPaths}
          {#if diffPaths.length}
            <table class="tbl" style="margin-top:var(--s2)">
              <thead><tr><th>changed path</th><th>emitted</th><th>now</th></tr></thead>
              <tbody>{#each diffPaths as [p, a, b]}<tr><td class="mono">{p}</td><td class="mono is-dim">{fmt.cut(a, 40)}</td><td class="mono is-warn">{fmt.cut(b, 40)}</td></tr>{/each}</tbody>
            </table>
          {:else}
            <p class="sm is-ok">Identical: the parsers loaded now produce exactly what was emitted.</p>
          {/if}
        {/if}
      </div>
    </div>
  </section>
{/if}
