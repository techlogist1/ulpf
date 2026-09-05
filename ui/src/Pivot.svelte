<script>
  import { api, fmt, summarize } from './api.js'
  import { live, trail, pushTrail } from './state.svelte.js'
  import { keys, nav } from './keys.js'
  import VList from './VList.svelte'

  let { kind = '', value = '' } = $props()

  const FALLBACK = ['src_ip', 'dst_ip', 'user', 'dst_port', 'device']
  let searchKind = $state('src_ip')
  let q = $state('')
  let entities = $state(null)
  let entErr = $state(null)
  let data = $state(null)
  let err = $state(null)
  let busy = $state(false)
  let sel = $state(-1)
  let box = $state(null)
  let innerHeight = $state(800)

  const paths = $derived(live.status?.schema?.entities ?? null)
  // The kinds the mapping declares, in the order an investigator asks them: source,
  // destination, who, where; the fixed five until status has answered.
  const KINDS = $derived.by(() => {
    if (!paths) return FALLBACK
    const declared = Object.keys(paths)
    return [...FALLBACK.filter((k) => declared.includes(k)), ...declared.filter((k) => !FALLBACK.includes(k))]
  })
  $effect(() => { if (KINDS.length && !KINDS.includes(searchKind)) searchKind = KINDS[0] })

  async function search() {
    const u = `/api/entities?kind=${encodeURIComponent(searchKind)}&limit=50${q ? `&q=${encodeURIComponent(q)}` : ''}`
    const r = await api('GET', u)
    if (r.ok) { entities = r.data.entities ?? []; entErr = null } else { entities = []; entErr = r.data }
  }
  $effect(() => { searchKind; q; if (!kind) search() })

  async function load(k, v, before, beforeId) {
    busy = true
    if (!before) { err = null; sel = -1 }
    const u = `/api/pivot?kind=${encodeURIComponent(k)}&value=${encodeURIComponent(v)}&limit=200${before ? `&before=${before}` : ''}${before && beforeId != null ? `&before_id=${beforeId}` : ''}`
    const r = await api('GET', u)
    busy = false
    if (!r.ok) { if (!before) { data = null; err = r.data } return }
    data = before ? { ...r.data, events: [...data.events, ...r.data.events] } : r.data
  }
  $effect(() => {
    if (kind && value) { pushTrail(kind, value); load(kind, value, null) }
    else { data = null; err = null }
  })

  const rows = $derived(data?.events ?? [])
  // The index need not carry the emitted line. Say so rather than showing blank columns.
  const hasLines = $derived(rows.some((r) => r.line))
  const noLine = $derived(rows.filter((r) => !r.line).length)
  const devColour = $derived.by(() => {
    const m = new Map()
    for (const d of data?.devices ?? []) m.set(d.device, `var(--p${m.size % 8})`)
    return m
  })
  const tint = (d) => devColour.get(d) ?? 'var(--line-3)'
  const pivot = (k, v) => (location.hash = `#/pivot/${encodeURIComponent(k)}/${encodeURIComponent(v)}`)

  // One lane per device over the loaded window: where in time each device saw this entity.
  // Ticks closer than one pixel would be are merged and drawn denser, so 200 events at
  // the same second read as one mark, not a smear.
  const lanes = $derived.by(() => {
    if (!rows.length) return null
    let lo = Infinity, hi = -Infinity
    for (const r of rows) { if (r.time < lo) lo = r.time; if (r.time > hi) hi = r.time }
    const span = hi - lo || 1
    const by = new Map()
    for (const r of rows) {
      const x = ((r.time - lo) / span) * 100
      const a = by.get(r.device) ?? []
      const last = a[a.length - 1]
      if (last && x - last.x < 0.15) { last.n++; last.dense = true } else a.push({ x, id: r.raw_id, t: r.time, n: 1, dense: false })
      by.set(r.device, a)
    }
    const step = span / 4
    const ticks = [0, 1, 2, 3, 4].map((i) => ({ x: i * 25, t: lo + i * step }))
    return { lo, hi, ticks, sameDay: fmt.day(lo) === fmt.day(hi), devices: [...by.entries()].sort((a, b) => b[1].reduce((s, t) => s + t.n, 0) - a[1].reduce((s, t) => s + t.n, 0)) }
  })
  const tickLabel = (t) => (lanes?.sameDay ? fmt.clock(t).slice(0, 8) : fmt.time(t).slice(5, 16))

  $effect(() => keys((e) => {
    if (e.key === '/') { box?.focus(); box?.select(); return true }
    if (!kind) return nav(e, entities?.length ?? 0, sel, (n) => (sel = n), (n) => pivot(entities[n].kind, entities[n].value))
    if (e.key === 'Escape') { location.hash = '#/pivot'; return true }
    if (e.key === 'Backspace') {
      const back = trail.steps[trail.steps.length - 2]
      location.hash = back ? `#/pivot/${encodeURIComponent(back.kind)}/${encodeURIComponent(back.value)}` : '#/pivot'
      return true
    }
    if (e.key === 'm' && data?.next_before) { load(kind, value, data.next_before, data.next_before_id); return true }
    return nav(e, rows.length, sel, (n) => (sel = n), (n) => (location.hash = `#/trace/${rows[n].raw_id}`))
  }))
  const deny = (a) => a === 'Denied' || a === 'Blocked' || a === 'Dropped'
</script>

<svelte:window bind:innerHeight />

{#if !kind}
  <section>
    <div class="head">
      <h2>Pivot</h2>
      <span class="note">one entity, every device that saw it</span>
    </div>
    <div class="bar">
      <span class="kinds" role="radiogroup" aria-label="Entity kind">
        {#each KINDS as k}
          <button class:on={searchKind === k} onclick={() => (searchKind = k)} role="radio" aria-checked={searchKind === k}>{k}</button>
        {/each}
      </span>
      <input type="search" bind:value={q} bind:this={box} placeholder="prefix, e.g. 203.0.113  /" size="26" aria-label="Entity prefix" />
      <span class="muted sm mono">{paths ? paths[searchKind] : ''}</span>
    </div>
  </section>

  <section>
    {#if entErr}
      <div class="notice bad">
        <b>{entErr.error}</b>
        <span class="muted">{entErr.reason}{#if entErr.status === 404}: the entity index is off on this server (ulpf serve --pivot on){/if}</span>
      </div>
    {:else if !entities}
      <p class="loading">reading the entity index</p>
    {:else if !entities.length}
      <div class="empty">
        <b>{q ? `No ${searchKind} starts with ${q}.` : 'No entities indexed yet.'}</b>
        <span>{q ? 'Shorten the prefix or pick another kind.' : 'The index fills as events are emitted; every event contributes its device, and the addresses, user and port the mapping names as entities.'}</span>
      </div>
    {:else}
      <div class="head quiet"><h3>{fmt.n(entities.length)} {searchKind}{q ? ` starting with ${q}` : ''}, most events first</h3><span class="note">Enter pivots on the selected row</span></div>
      <div style="--cols:7em minmax(0,1fr) 8em 6em 13em 13em">
        <VList items={entities} max={Math.max(396, innerHeight - 260)} {sel}>
          {#snippet header()}<div class="vh"><span>kind</span><span>value</span><span class="num">events</span><span class="num">devices</span><span>first seen</span><span>last seen</span></div>{/snippet}
          {#snippet row(e, i)}
            <div class="vr" class:sel={i === sel} onclick={() => pivot(e.kind, e.value)} role="button" tabindex="-1">
              <span class="is-dim">{e.kind}</span>
              <span class="mono">{e.value}</span>
              <span class="num">{fmt.n(e.events)}</span>
              <span class="num">{fmt.n(e.devices)}</span>
              <span class="mono is-dim">{fmt.time(e.first_time).slice(0, 19)}</span>
              <span class="mono is-dim">{fmt.time(e.last_time).slice(0, 19)}</span>
            </div>
          {/snippet}
        </VList>
      </div>
    {/if}
  </section>
{:else}
  <section class="stack">
    <div class="trail">
      <a href="#/pivot">Pivot</a>
      {#each trail.steps as s, i}
        <span class="sep">/</span>
        {#if i === trail.steps.length - 1}
          <span class="cur">{s.kind} {s.value}</span>
        {:else}
          <a href="#/pivot/{encodeURIComponent(s.kind)}/{encodeURIComponent(s.value)}">{s.kind} {s.value}</a>
        {/if}
      {/each}
      {#if trail.steps.length > 1}<span class="back">Backspace steps back</span>{/if}
    </div>

    {#if err}
      <div class="notice bad">
        <b>{err.error}</b>
        <span class="muted">{err.reason}{#if err.status === 422}: {kind} is not an entity kind the mapping declares{/if}</span>
        <span><a href="#/pivot">Back to the entity search</a></span>
      </div>
    {:else if !data}
      <p class="loading">reading the posting list for {kind} {value}</p>
    {:else if !data.total}
      <div class="entity"><span class="kind">{kind}</span><span class="val">{value}</span></div>
      <div class="empty">
        <b>No event carries this value.</b>
        <span>The index has nothing under {kind} = {value}. It may have arrived under another kind (a source address seen only as a destination), or not at all.</span>
        <span><a href="#/pivot">Search the index</a></span>
      </div>
    {:else}
      <div class="entity">
        <span class="kind">{kind}</span>
        <span class="val">{value}</span>
        <span class="facts" style="margin-left:var(--s6)">
          <div><span>events</span><b>{fmt.n(data.total)}</b></div>
          <div><span>devices</span><b>{fmt.n(data.devices?.length)}</b></div>
          <div><span>first</span><b>{fmt.time(data.first_time)}</b></div>
          <div><span>last</span><b>{fmt.time(data.last_time)}</b></div>
        </span>
      </div>

      {#if lanes}
        <div class="lanes">
          <div class="head">
            <h2>Across devices</h2>
            <span class="note">the {fmt.n(rows.length)} loaded events placed in time, one lane per device, most events first</span>
            {#if data.next_before}<span class="push"><button class="btn" onclick={() => load(kind, value, data.next_before, data.next_before_id)} disabled={busy}>Load older<kbd>m</kbd></button></span>{/if}
          </div>
          {#each lanes.devices as [dev, ticks]}
            {@const info = (data.devices ?? []).find((d) => d.device === dev)}
            <div class="lane" style="--c:{tint(dev)}">
              <span class="lname" title="{dev}: {fmt.n(info?.events ?? ticks.length)} events in total, parsers {info?.parsers?.join(', ') || 'none'}">{dev}</span>
              <span class="ltrack">
                {#each ticks as t}<i class:dense={t.dense} style="left:{t.x}%" title="{fmt.time(t.t)}  {t.n > 1 ? `${t.n} events` : `raw ${t.id}`}"></i>{/each}
              </span>
              <span class="lcount">{fmt.n(ticks.reduce((s, t) => s + t.n, 0))}</span>
            </div>
          {/each}
          <div class="axis">
            <span></span>
            <span class="ticks">
              {#each lanes.ticks as t, i}<span class="tick" class:first={i === 0} class:last={i === lanes.ticks.length - 1} style="left:{t.x}%">{tickLabel(t.t)}</span>{/each}
            </span>
            <span class="lcount xs muted">{lanes.sameDay ? fmt.day(lanes.lo) : ''}</span>
          </div>
        </div>
      {/if}

      <div class="split pivot">
        <div>
          <div class="head">
            <h2>Timeline</h2>
            <span class="note">newest first, {fmt.n(rows.length)} of {fmt.n(data.total)} loaded, Enter traces the selected event</span>
            {#if noLine}<span class="note">{hasLines ? `${fmt.n(noLine)} rows have left the tail; open one for the stored record` : 'the index carries no emitted line; open a row for the record'}</span>{/if}
          </div>
          <div style="--cols:11em 10em 9em{hasLines ? ' 8em 6em minmax(0,1fr)' : ' minmax(0,1fr)'} 5em">
            <VList items={rows} max={Math.max(396, innerHeight - 520)} {sel}>
              {#snippet header()}<div class="vh"><span>time</span><span>device</span><span>parser</span>{#if hasLines}<span>class</span><span>action</span><span>what</span>{:else}<span></span>{/if}<span class="num">raw</span></div>{/snippet}
              {#snippet row(ev, i)}
                <div class="vr" class:sel={i === sel} onclick={() => (location.hash = `#/trace/${ev.raw_id}`)} role="button" tabindex="-1">
                  <span class="mono is-dim">{fmt.stamp(ev.time)}</span>
                  <span class="mono dev" style="--c:{tint(ev.device)}">{ev.device}</span>
                  <span class="mono is-dim">{ev.parser ?? 'none'}</span>
                  {#if hasLines}
                    <span>{ev.line?.class_name ?? ''}</span>
                    <span class:is-warn={deny(ev.line?.action)}>{ev.line?.action ?? ''}</span>
                    <span class="mono">{#if ev.line}{summarize(ev.line)}{:else}<span class="is-dim">record only</span>{/if}</span>
                  {:else}
                    <span></span>
                  {/if}
                  <span class="num">{ev.raw_id}</span>
                </div>
              {/snippet}
            </VList>
          </div>
          {#if data.next_before}
            <div class="bar" style="margin-top:var(--s3)"><button class="btn" onclick={() => load(kind, value, data.next_before, data.next_before_id)} disabled={busy}>Load older<kbd>m</kbd></button><span class="muted sm">{fmt.n(data.total - rows.length)} older events not loaded</span></div>
          {/if}
        </div>

        <div class="related">
          <div class="head"><h2>Seen with</h2><span class="note">the ten most frequent per kind over the newest {fmt.n(data.related_over ?? data.total)} events; click to pivot</span></div>
          {#each KINDS as k}
            {@const items = data.related?.[k] ?? []}
            {#if items.length}
              {@const top = items[0].events || 1}
              <div>
                <h3>{k}</h3>
                <ul>
                  {#each items as r}
                    <li>
                      <a href="#/pivot/{encodeURIComponent(k)}/{encodeURIComponent(r.value)}" class:dev={k === 'device'} style={k === 'device' ? `--c:${tint(r.value)}` : ''}>{r.value}</a>
                      <span class="share"><i style="width:{(100 * r.events) / top}%"></i></span>
                      <span class="n">{fmt.n(r.events)}</span>
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}
          {/each}
          {#if !Object.values(data.related ?? {}).some((v) => v.length)}
            <div class="empty"><b>Nothing co-occurs with this entity yet.</b><span>Related values come from the other entity fields of the same events.</span></div>
          {/if}
        </div>
      </div>
    {/if}
  </section>
{/if}
