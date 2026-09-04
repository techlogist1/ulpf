<script>
  import { api, fmt } from './api.js'

  let { id = '' } = $props()
  let input = $state('')
  let data = $state(null)
  let err = $state(null)
  let busy = $state(false)

  async function load(rid) {
    if (rid === '' || rid == null) return
    busy = true; err = null; data = null
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

  // 16 bytes per row: offset, hex pairs, printable ASCII.
  function dump(hex) {
    const rows = []
    for (let i = 0; i < hex.length; i += 32) {
      const chunk = hex.slice(i, i + 32)
      const pairs = chunk.match(/../g) ?? []
      const ascii = pairs.map((p) => { const c = parseInt(p, 16); return c >= 0x20 && c < 0x7f ? String.fromCharCode(c) : '.' }).join('')
      rows.push(`${(i / 2).toString(16).padStart(8, '0')}  ${pairs.join(' ').padEnd(47)}  ${ascii}`)
    }
    return rows.join('\n')
  }
</script>

<section>
  <h2>Traceback</h2>
  <form class="bar" onsubmit={go}>
    <label for="rid">raw id</label>
    <input id="rid" type="text" inputmode="numeric" bind:value={input} placeholder="e.g. 4211" class="mono" />
    <button class="btn primary" type="submit" disabled={busy}>Look up</button>
    {#if busy}<span class="muted sm">loading…</span>{/if}
  </form>
</section>

{#if err}
  <div class="notice bad">
    <b>{err.error}</b> <span class="muted">({err.reason})</span>
    {#if err.store_len != null}<p class="sm">The store holds {fmt.n(err.store_len)} records; ids run from 0 to {fmt.n(Math.max(0, err.store_len - 1))}.</p>{/if}
  </div>
{:else if !data && !id}
  <p class="empty">Enter a raw id, or click a row in Live to trace it. The exact stored bytes are shown with their digest re-checked now.</p>
{:else if data}
  <section class="stack">
    <div class="facts">
      <div><span>raw_id</span><b class="mono">{data.raw_id}</b></div>
      <div><span>source</span><b class="mono">{data.source}</b></div>
      <div><span>receipt</span><b class="mono">{data.receipt}</b></div>
      <div><span>bytes</span><b class="mono">{fmt.n(data.bytes_len)}</b></div>
    </div>

    <div class="digests">
      <div class="d">
        <div class="sm muted">stored SHA-256</div>
        <pre>{data.stored_sha256}</pre>
      </div>
      <div class="d">
        <div class="sm muted">recomputed now</div>
        <pre>{data.recomputed_sha256}</pre>
      </div>
    </div>
    <p class={data.digest_match ? 'match' : 'mismatch'}>{data.digest_match ? 'Digests match: the stored bytes are unchanged.' : 'Digests differ: the stored bytes do not hash to the recorded digest.'}</p>

    <h3>Text</h3>
    <pre class="box">{data.text}</pre>

    <h3>Bytes</h3>
    <pre class="box">{dump(data.hex)}</pre>

    <div class="two">
      <div>
        <h3>Emitted <span class="muted">as written to the output</span></h3>
        {#if data.emitted}
          <pre class="box">{fmt.json(data.emitted)}</pre>
        {:else}
          <p class="empty">Not in the tail any more; the output file has the emitted line.</p>
        {/if}
      </div>
      <div>
        <h3>Re-normalized now <span class="muted">parser {data.now?.parser ?? 'none'}, {data.now?.parse_status}</span></h3>
        <pre class="box">{fmt.json(data.now?.normalized ?? {})}</pre>
      </div>
    </div>
  </section>
{/if}
