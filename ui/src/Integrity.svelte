<script>
  import { api, fmt } from './api.js'
  import { live } from './state.svelte.js'
  import { keys } from './keys.js'
  import Confirm from './Confirm.svelte'

  let data = $state(null)
  let err = $state(null)
  let busy = $state(false)
  let note = $state(null)
  let asking = $state(false)

  async function load() {
    const r = await api('GET', '/api/integrity')
    if (r.ok) { data = r.data; err = null } else err = r.data
  }
  load()
  $effect(() => { live.integrity; load() })
  // the started notice is about a run in flight; the result below replaces it
  $effect(() => { if (data && !data.running && data.last_verify && note?.kind === 'ok') note = null })

  async function verify() {
    asking = false
    busy = true; note = null
    const r = await api('POST', '/api/integrity/verify')
    busy = false
    if (r.ok) note = { kind: 'ok', text: `Verifying ${fmt.n(r.data.records)} records on a snapshot of the store.` }
    else note = { kind: 'bad', text: `${r.data.error} (${r.data.reason})` }
    load()
  }
  const canVerify = $derived(!busy && data && !data.running && data.records > 0)
  $effect(() => keys((e) => {
    if (e.key === 'v' && canVerify && !asking) { asking = true; return true }
    return false
  }))

  const v = $derived(data?.last_verify)
</script>

<section>
  <div class="head">
    <h2>Integrity</h2>
    <span class="note">every record's digest chained to the one before it: chain = sha256(prev_chain ‖ digest)</span>
    <span class="push bar">
      <a class="btn" href="/api/integrity/attestation" target="_blank" rel="noreferrer">Export attestation</a>
      <button class="btn primary" onclick={() => (asking = true)} disabled={!canVerify}>{data?.running ? 'Verifying' : 'Verify the store'}<kbd>v</kbd></button>
    </span>
  </div>

  {#if err}
    <div class="notice bad"><b>{err.error}</b><span class="muted">{err.reason}</span></div>
  {:else if !data}
    <p class="loading">reading the chain head</p>
  {:else}
    <div class="stack">
      {#if asking}
        <Confirm title="Verify {fmt.n(data.records)} records?" hint="Every record's bytes are re-hashed and every chain link recomputed on a snapshot of the store, on its own thread. The engine keeps ingesting. A store this size takes seconds to a minute." verb="Verify" onconfirm={verify} oncancel={() => (asking = false)} />
      {/if}
      {#if note}<div class="notice {note.kind}"><b>{note.text}</b></div>{/if}

      {#if data.records === 0}
        <div class="empty">
          <b>The store is empty.</b>
          <span>The genesis value below was fixed when the store was created; the head appears with the first record, and a verify has something to check once events arrive.</span>
        </div>
      {/if}

      <div class="verdicts">
        {#if data.running}
          <div class="verdict warn">
            <b>Verify running</b>
            <span class="lab">on a snapshot of the store, on its own thread; the result replaces this panel</span>
            <div class="meter busy"><i></i></div>
          </div>
        {:else if v}
          <div class="verdict" class:ok={v.ok} class:bad={!v.ok}>
            <b>{v.ok ? `Clean: ${fmt.n(v.records)} records recomputed, every chain value follows` : `Broken at raw id ${fmt.n(v.first_bad)}: the ${v.reason} does not match`}</b>
            <span class="lab">{fmt.stamp(v.at)}, {fmt.f(v.elapsed_secs, 2)}s, {fmt.n(v.corrupt)} corrupt, {v.against_attestation ? 'checked against the attestation document' : 'store-only check'}</span>
            {#if !v.ok}<span><a href="#/trace/{v.first_bad}">Trace record {fmt.n(v.first_bad)}</a> to read the stored bytes beside the digest that disagrees.</span>{/if}
          </div>
        {:else}
          <div class="verdict">
            <b>No verify has run in this session</b>
            <span class="lab">a verify recomputes every record's digest and chain value against the store; press v</span>
          </div>
        {/if}
        <div class="verdict">
          <b>{fmt.n(data.records)} records in the chain</b>
          <span class="lab">store id</span><pre>{data.store_id}</pre>
          <span class="lab">checkpoint every {fmt.n(data.checkpoint_every)} records in the attestation</span>
        </div>
      </div>

      <div class="proof chain">
        <span>genesis</span><b>{data.genesis}</b>
        <span class="muted xs" style="grid-column:2">sha256("ULPF chain genesis" ‖ store id)</span>
        <span>head</span><b class:is-dim={!data.head}>{data.head ?? 'none: the store is empty'}</b>
        <span class="muted xs" style="grid-column:2">the newest record's chain value; changes with every record</span>
      </div>

      <p class="sm muted">
        The attestation document holds the store id, the genesis, the head and every {fmt.n(data.checkpoint_every)}th chain value.
        A stranger with the store directory and that file runs <code>ulpf verify --store DIR --attestation FILE</code> offline:
        a store rewritten consistently from record N passes the store-only check and fails at the first checkpoint at or after N.
      </p>
    </div>
  {/if}
</section>
