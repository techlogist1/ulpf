<script>
  import { api, fmt, summarize, leaf } from './api.js'
  import { live, trail, pushTrail } from './state.svelte.js'
  import { keys, nav } from './keys.js'

  let { kind = '', value = '' } = $props()

  const FALLBACK = ['src_ip', 'dst_ip', 'user', 'dst_port', 'device']
  let searchKind = $state('src_ip')
  let q = $state('')
  let entities = $state([])
  let entErr = $state(null)
  let data = $state(null)
  let err = $state(null)
  let busy = $state(false)
  let more = $state(false)
  let sel = $state(-1)
  let box = $state(null)

  const paths = $derived(live.status?.schema?.entities ?? null)
  // The kinds the mapping declares; the fixed five until status has answered.
  // The declared kinds, read in the order an investigator asks them: source, destination, who.
  const KINDS = $derived.by(() => {
    if (!paths) return FALLBACK
    const declared = Object.keys(paths)
    return [...FALLBACK.filter((k) => declared.includes(k)), ...declared.filter((k) => !FALLBACK.includes(k))]
  })
  $effect(() => { if (KINDS.length && !KINDS.includes(searchKind)) searchKind = KINDS[0] })

  async function search() {
    const u = `/api/entities?kind=${encodeURIComponent(searchKind)}&limit=25${q ? `&q=${encodeURIComponent(q)}` : ''}`
    const r = await api('GET', u)
    if (r.ok) { entities = r.data.entities ?? []; entErr = null } else { entities = []; entErr = r.data }
  }
  $effect(() => { searchKind; q; if (!kind) search() })

  async function load(k, v, before, beforeId) {
    busy = true
    if (!before) { err = null; sel = -1 }
    const u = `/api/pivot?kind=${encodeURIComponent(k)}&value=${encodeURIComponent(v)}&limit=100${before ? `&before=${before}` : ''}${before && beforeId != null ? `&before_id=${beforeId}` : ''}`
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
  const pivot = (k, v) => (location.hash = `#/pivot/${encodeURIComponent(k)}/${encodeURIComponent(v)}`)

  // One lane per device over the loaded window: where in time each device saw this entity.
  const lanes = $derived.by(() => {
    if (!rows.length) return null
    let lo = Infinity, hi = -Infinity
    for (const r of rows) { if (r.time < lo) lo = r.time; if (r.time > hi) hi = r.time }
    const span = hi - lo || 1
    const by = new Map()
    for (const r of rows) {
      const a = by.get(r.device) ?? []
      a.push({ x: ((r.time - lo) / span) * 100, id: r.raw_id, t: r.time })
      by.set(r.device, a)
    }
    return { lo, hi, devices: [...by.entries()].sort((a, b) => b[1].length - a[1].length) }
  })

  $effect(() => keys((e) => {
    if (e.key === '/') { box?.focus(); box?.select(); return true }
    if (!kind) return nav(e, entities.length, sel, (n) => (sel = n), (n) => pivot(entities[n].kind, entities[n].value))
    if (e.key === 'Escape') { location.hash = '#/pivot'; return true }
    if (e.key === 'Backspace') {
      const back = trail.steps[trail.steps.length - 2]
      if (back) pivot(back.kind, back.value)
      return true
    }
    if (e.key === 'm' && data?.next_before) { load(kind, value, data.next_before, data.next_before_id); return true }
    return nav(e, rows.length, sel, (n) => (sel = n), (n) => (location.hash = `#/trace/${rows[n].raw_id}`))
  }))
</script>

{#if !kind}
  <section>
    <div class="head">
      <h2>Pivot</h2>
      <span class="note">one entity, every device that saw it</span>
    </div>
    <div class="bar">
      {#each KINDS as k}
        <button class="btn" class:on={searchKind === k} onclick={() => (searchKind = k)}>{k}</button>
      {/each}
      <input type="search" bind:value={q} bind:this={box} placeholder="prefix, e.g. 203.0.113" size="26" />
      <span class="muted sm">{paths ? paths[searchKind] : ''}</span>
    </div>
  </section>

  <section>
    {#if entErr}
      <p class="notice bad">{entErr.error} ({entErr.reason})</p>
    {:else if !entities.length}
      <p class="empty">{q ? `No ${searchKind} starts with ${q}.` : 'No entities indexed yet. The index fills as events are emitted.'}</p>
    {:else}
      <div class="wrap"><table class="tbl">
        <thead><tr><th>kind</th><th>value</th><th class="num">events</th><th class="num">devices</th><th>first seen</th><th>last seen</th><th class="fill"></th></tr></thead>
        <tbody>
          {#each entities as e, i (e.kind + e.value)}
            <tr class="click" class:sel={i === sel} onclick={() => pivot(e.kind, e.value)}>
              <td class="is-dim">{e.kind}</td>
              <td class="mono">{e.value}</td>
              <td class="num">{fmt.n(e.events)}</td>
              <td class="num">{fmt.n(e.devices)}</td>
              <td class="mono is-dim">{fmt.time(e.first_time)}</td>
              <td class="mono is-dim">{fmt.time(e.last_time)}</td>
              <td class="fill"></td>
            </tr>
          {/each}
        </tbody>
      </table></div>
    {/if}
  </section>
{:else}
  <section class="stack">
    <div class="trail">
      <a href="#/pivot">search</a>
      {#each trail.steps as s, i}
        <span class="sep">/</span>
        {#if i === trail.steps.length - 1}
          <span class="cur">{s.kind} {s.value}</span>
        {:else}
          <a href="#/pivot/{encodeURIComponent(s.kind)}/{encodeURIComponent(s.value)}">{s.kind} {s.value}</a>
        {/if}
      {/each}
    </div>

    {#if err}
      <p class="notice bad">{err.error} ({err.reason})</p>
    {:else if !data}
      <p class="empty">Loading the timeline.</p>
    {:else}
      <div class="facts">
        <div><span>{kind}</span><b>{value}</b></div>
        <div><span>events</span><b>{fmt.n(data.total)}</b></div>
        <div><span>first</span><b>{fmt.time(data.first_time)}</b></div>
        <div><span>last</span><b>{fmt.time(data.last_time)}</b></div>
        <div><span>devices</span><b>{fmt.n(data.devices?.length)}</b></div>
      </div>

      <div class="devbar">
        {#each data.devices ?? [] as d}
          <div class="dev" style="--dev:{devColour.get(d.device)}" title="parsers: {d.parsers?.join(', ') || 'none'}">
            <b>{d.device}</b><span class="n">{fmt.n(d.events)}</span>
          </div>
        {/each}
      </div>

      {#if lanes}
        <div class="lanes">
          <div class="head"><h2>Across devices</h2><span class="note">the {fmt.n(rows.length)} loaded events placed in time, one lane per device</span></div>
          {#each lanes.devices as [dev, ticks]}
            <div class="lane">
              <span class="lname dev" style="--dev:{devColour.get(dev) ?? 'var(--rule-strong)'}">{dev}</span>
              <span class="ltrack">
                {#each ticks as t}<i style="left:{t.x}%; background:{devColour.get(dev) ?? 'var(--rule-strong)'}" title="{fmt.time(t.t)}  raw {t.id}"></i>{/each}
              </span>
              <span class="lcount num">{fmt.n(ticks.length)}</span>
            </div>
          {/each}
          <div class="lscale"><span>{fmt.time(lanes.lo)}</span><span class="push">{fmt.time(lanes.hi)}</span></div>
        </div>
      {/if}

      <div class="split">
        <div>
          <div class="head">
            <h2>Timeline</h2>
            <span class="note">newest first, {fmt.n(rows.length)} of {fmt.n(data.total)} loaded</span>
            {#if noLine}<span class="note is-dim">{hasLines ? `${fmt.n(noLine)} rows have left the tail; open one for the stored record` : 'the index carries no emitted line; open a row for the record'}</span>{/if}
            <span class="push">
              {#if data.next_before}<button class="btn" onclick={() => load(kind, value, data.next_before, data.next_before_id)} disabled={busy}>Load older</button>{/if}
            </span>
          </div>
          <div class="scroll">
            <table class="tbl">
              <thead><tr><th>time</th><th>device</th><th>parser</th>{#if hasLines}<th>class</th><th>action</th><th>what</th>{/if}<th class="num">raw</th></tr></thead>
              <tbody>
                {#each rows as ev, i (ev.raw_id)}
                  <tr class="click" class:sel={i === sel} onclick={() => (location.hash = `#/trace/${ev.raw_id}`)}>
                    <td class="mono is-dim">{fmt.time(ev.time)}</td>
                    <td class="mono dev" style="--dev:{devColour.get(ev.device) ?? 'var(--rule-strong)'}">{ev.device}</td>
                    <td class="mono is-dim">{ev.parser ?? 'none'}</td>
                    {#if hasLines}
                      <td>{ev.line?.class_name ?? ''}</td>
                      <td class:is-warn={ev.line?.action === 'Denied' || ev.line?.action === 'Blocked'}>{ev.line?.action ?? ''}</td>
                      <td class="mono ell">{#if ev.line}{fmt.cut(summarize(ev.line), 120)}{:else}<span class="is-dim">—</span>{/if}</td>
                    {/if}
                    <td class="num">{ev.raw_id}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </div>

        <div class="related">
          <div class="head"><h2>Seen with</h2><span class="note">over the newest {fmt.n(data.related_over ?? data.total)} events</span></div>
          {#each KINDS as k}
            {@const items = data.related?.[k] ?? []}
            {#if items.length}
              <div>
                <h3>{k}</h3>
                <ul>
                  {#each items as r}
                    <li><a href="#/pivot/{encodeURIComponent(k)}/{encodeURIComponent(r.value)}">{r.value}</a><span class="n">{fmt.n(r.events)}</span></li>
                  {/each}
                </ul>
              </div>
            {/if}
          {/each}
          {#if !Object.values(data.related ?? {}).some((v) => v.length)}
            <p class="empty">Nothing co-occurs with this entity yet.</p>
          {/if}
        </div>
      </div>
    {/if}
  </section>
{/if}
