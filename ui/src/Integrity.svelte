<script>
  import { api, fmt } from './api.js'
  import { live } from './state.svelte.js'

  let data = $state(null)
  let err = $state(null)
  let busy = $state(false)
  let note = $state(null)

  async function load() {
    const r = await api('GET', '/api/integrity')
    if (r.ok) { data = r.data; err = null } else err = r.data
  }
  load()
  $effect(() => { live.integrity; load() })

  async function verify() {
    busy = true; note = null
    const r = await api('POST', '/api/integrity/verify')
    busy = false
    if (r.ok) note = { kind: 'ok', text: `Verifying ${fmt.n(r.data.records)} records on a snapshot of the store.` }
    else note = { kind: 'bad', text: `${r.data.error} (${r.data.reason})` }
    load()
  }

  const v = $derived(data?.last_verify)
</script>

<section>
  <div class="head">
    <h2>Integrity</h2>
    <span class="note">every record's digest chained to the one before it: chain = sha256(prev_chain ‖ digest)</span>
    <span class="push bar">
      <a class="btn" href="/api/integrity/attestation" target="_blank" rel="noreferrer">Export attestation</a>
      <button class="btn primary" onclick={verify} disabled={busy || data?.running}>{data?.running ? 'Verifying…' : 'Verify the store'}</button>
    </span>
  </div>

  {#if err}
    <p class="notice bad">{err.error} ({err.reason})</p>
  {:else if !data}
    <p class="empty">Loading.</p>
  {:else}
    <div class="stack">
      {#if note}<p class="notice {note.kind}">{note.text}</p>{/if}

      <div class="counters">
        <div class="crow">
          <b>store</b>
          <span class="kvs">
            <span class="kv big"><span>records</span><span class="num">{fmt.n(data.records)}</span></span>
            <span class="kv"><span>store id</span><span class="num">{data.store_id}</span></span>
            <span class="kv"><span>checkpoint every</span><span class="num">{fmt.n(data.checkpoint_every)}</span></span>
          </span>
        </div>
      </div>

      <div class="chain">
        <div class="d"><span class="lab">genesis — sha256("ULPF chain genesis" ‖ store id)</span><pre>{data.genesis}</pre></div>
        <div class="d"><span class="lab">head — the newest record's chain value</span><pre>{data.head ?? 'the store is empty'}</pre></div>
      </div>

      {#if data.running}
        <div class="panel pad"><div class="meter"><i style="width:100%"></i></div><p class="sm muted">A verify is running on its own thread over a snapshot of the store.</p></div>
      {:else if v}
        <div class="notice {v.ok ? 'ok' : 'bad'}">
          <b>{v.ok ? `Clean: ${fmt.n(v.records)} records recomputed and every chain value follows` : `Broken at raw id ${v.first_bad} (${v.reason})`}</b>
          <p class="sm muted">
            {v.at}, {fmt.f(v.elapsed_secs, 2)}s, {fmt.n(v.corrupt)} corrupt{v.against_attestation ? ', checked against the attestation document' : ', store-only check'}
          </p>
          {#if !v.ok}
            <p class="sm"><a href="#/trace/{v.first_bad}">Trace record {v.first_bad}</a> to see the stored bytes and the digest that disagrees.</p>
          {/if}
        </div>
      {:else}
        <p class="empty">No verify has run in this session. A verify recomputes every record's digest and chain value against the store.</p>
      {/if}

      <p class="sm muted">
        The attestation document holds the store id, the genesis, the head and every 4,096th chain value.
        A stranger with the store directory and that file runs <code>ulpf verify --store DIR --attestation FILE</code> offline:
        a store rewritten consistently from record N passes the store-only check and fails at the first checkpoint at or after N.
      </p>
    </div>
  {/if}
</section>
