<script>
  import { api, fmt } from './api.js'
  import { live } from './state.svelte.js'
  import { keys, nav } from './keys.js'

  let data = $state(null)
  let err = $state(null)
  let busy = $state(false)
  let started = $state(null)
  let entries = $state([])
  let kindFilter = $state('')
  let nextAfter = $state(null)
  let diffVersion = $state(null)
  let sel = $state(-1)

  async function load() {
    const r = await api('GET', '/api/replay')
    if (r.ok) { data = r.data; err = null } else err = r.data
  }
  load()
  // The SSE replay frame drives progress; every state change re-reads the versions list.
  $effect(() => { live.replay?.state; load() })

  const running = $derived(live.replay?.state === 'progress' ? live.replay : data?.running)
  const report = $derived(data?.last ?? (live.replay?.report ?? null))

  async function start() {
    busy = true
    const r = await api('POST', '/api/replay', {})
    busy = false
    if (r.ok) { started = r.data; load() }
    else err = r.data
  }

  async function loadDiff(version, after = null) {
    diffVersion = version
    const u = `/api/replay/${version}/diff?limit=100${after != null ? `&after=${after}` : ''}${kindFilter ? `&kind=${kindFilter}` : ''}`
    const r = await api('GET', u)
    if (!r.ok) { err = r.data; entries = []; return }
    entries = after == null ? r.data.entries : [...entries, ...r.data.entries]
    nextAfter = r.data.next_after
  }
  $effect(() => { kindFilter; if (diffVersion) loadDiff(diffVersion, null) })
  $effect(() => keys((e) => nav(e, entries.length, sel, (n) => (sel = n), (n) => (location.hash = `#/trace/${entries[n].raw_id}`))))

  const pairs = (o) => Object.entries(o ?? {})
  const base = (p) => String(p ?? '').split('/').pop()
</script>

<section>
  <div class="head">
    <h2>Replay</h2>
    <span class="note">the same stored bytes through today's parsers, versioned beside the live output</span>
    <span class="push bar">
      {#if running}
        <span class="tag warn">v{running.version} running</span>
      {/if}
      <button class="btn primary" onclick={start} disabled={busy || !!running}>Replay the store</button>
    </span>
  </div>

  {#if err}<p class="notice bad">{err.error} ({err.reason})</p>{/if}
  {#if data?.last_error}<p class="notice bad"><b>The last replay failed</b> <span class="mono">{data.last_error}</span></p>{/if}
  {#if started && !running && report?.version !== started.version}<p class="notice ok">Started version {started.version} over {fmt.n(started.total)} records.</p>{/if}

  {#if running}
    <div class="panel pad stack">
      <div class="bar sm"><b class="mono">v{running.version}</b><span class="muted">{fmt.n(running.done)} of {fmt.n(running.total)} records</span></div>
      <div class="meter"><i style="width:{running.total ? (100 * running.done) / running.total : 0}%"></i></div>
    </div>
  {/if}
</section>

<section>
  <div class="head"><h2>Versions</h2><span class="note">version 1 is the live output</span></div>
  {#if !data?.versions?.length}
    <p class="empty">No output versions yet.</p>
  {:else}
    <div class="wrap"><table class="tbl">
      <thead><tr><th>version</th><th>path</th><th>created</th><th class="num">events</th><th>schema</th><th class="num">parsers gen</th><th>diff</th><th class="fill"></th></tr></thead>
      <tbody>
        {#each data.versions as v (v.version)}
          <tr>
            <td class="mono">v{v.version}</td>
            <td class="mono is-dim" title={v.path}>{base(v.path)}</td>
            <td class="mono is-dim">{v.created}</td>
            <td class="num">{fmt.n(v.events)}</td>
            <td>{v.schema}</td>
            <td class="num">{fmt.n(v.parsers_generation)}</td>
            <td>{#if v.version > 1}<button class="btn" class:on={diffVersion === v.version} onclick={() => loadDiff(v.version, null)}>Open diff</button>{:else}<span class="is-dim">baseline</span>{/if}</td>
            <td class="fill"></td>
          </tr>
        {/each}
      </tbody>
    </table></div>
  {/if}
</section>

{#if report}
  <section class="stack">
    <div class="head"><h2>Report for v{report.version}</h2><span class="note">against v{report.previous_version ?? '–'}, {fmt.n(report.events)} events in {fmt.f(report.elapsed_secs, 2)}s at {fmt.n(Math.round(report.events_per_sec))}/s</span></div>

    <div class="why">
      <h3>Why the output changed</h3>
      {#each report.why ?? [] as w}<p>{w}</p>{:else}<p class="is-dim">The report carries no explanation.</p>{/each}
      <div class="bar sm muted">
        <span>output <span class="mono" title={report.output}>{base(report.output)}</span></span>
        {#if report.diff}<span>diff <span class="mono" title={report.diff}>{base(report.diff)}</span></span>{/if}
      </div>
    </div>

    <div class="counters">
      <div class="crow">
        <b>events</b>
        <span class="kvs">
          <span class="kv"><span>unchanged</span><span class="num">{fmt.n(report.summary?.unchanged)}</span></span>
          <span class="kv on"><span>changed</span><span class="num">{fmt.n(report.summary?.changed)}</span></span>
          <span class="kv ok"><span>only in new</span><span class="num">{fmt.n(report.summary?.only_in_new)}</span></span>
          <span class="kv bad"><span>only in old</span><span class="num">{fmt.n(report.summary?.only_in_old)}</span></span>
        </span>
      </div>
      <div class="crow">
        <b>fields</b>
        <span class="kvs">
          <span class="kv ok"><span>added</span><span class="num">{fmt.n(report.summary?.fields_added)}</span></span>
          <span class="kv bad"><span>lost</span><span class="num">{fmt.n(report.summary?.fields_lost)}</span></span>
          <span class="kv on"><span>changed</span><span class="num">{fmt.n(report.summary?.fields_changed)}</span></span>
          <span class="kv"><span>parsers generation</span><span class="num">{fmt.n(report.parsers_generation)}</span></span>
        </span>
      </div>
    </div>

    <div class="split">
      <div>
        <div class="head"><h2>Parser changes</h2></div>
        <table class="tbl">
          <thead><tr><th>before</th><th>after</th><th class="num">events</th></tr></thead>
          <tbody>
            {#each report.summary?.parser_changes ?? [] as c}
              <tr>
                <td class="mono">{c.from ?? '—'}{#if !c.from}<span class="tag warn"> unparsed</span>{/if}</td>
                <td class="mono">{c.to ?? '—'}</td>
                <td class="num">{fmt.n(c.events)}</td>
              </tr>
            {:else}
              <tr><td colspan="3" class="is-dim">No event changed the parser that claimed it.</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
      <div>
        <div class="head"><h2>By field</h2></div>
        <table class="tbl">
          <thead><tr><th>path</th><th class="num">added</th><th class="num">lost</th><th class="num">changed</th></tr></thead>
          <tbody>
            {#each report.summary?.by_field ?? [] as f}
              <tr>
                <td class="mono">{f.path}</td>
                <td class="num is-ok">{fmt.n(f.added)}</td>
                <td class="num is-bad">{fmt.n(f.lost)}</td>
                <td class="num is-warn">{fmt.n(f.changed)}</td>
              </tr>
            {:else}
              <tr><td colspan="4" class="is-dim">No schema field gained, lost or changed a value.</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>
  </section>
{/if}

{#if diffVersion}
  <section>
    <div class="head">
      <h2>Diff entries for v{diffVersion}</h2>
      <span class="note">{fmt.n(entries.length)} loaded</span>
      <span class="push bar">
        {#each [['', 'all'], ['changed', 'changed'], ['only_in_new', 'only in new'], ['only_in_old', 'only in old']] as [k, label]}
          <button class="btn" class:on={kindFilter === k} onclick={() => (kindFilter = k)}>{label}</button>
        {/each}
        {#if nextAfter != null}<button class="btn" onclick={() => loadDiff(diffVersion, nextAfter)}>Load more</button>{/if}
      </span>
    </div>
    {#if !entries.length}
      <p class="empty">No entries of this kind.</p>
    {:else}
      <div class="scroll">
        <table class="tbl">
          <thead><tr><th class="num">raw</th><th>kind</th><th>parser before</th><th>parser after</th><th>added</th><th>lost</th><th>changed</th></tr></thead>
          <tbody>
            {#each entries as e, i (e.raw_id + e.kind)}
              <tr class="click" class:sel={i === sel} onclick={() => (location.hash = `#/trace/${e.raw_id}`)}>
                <td class="num">{e.raw_id}</td>
                <td><span class="tag" class:ok={e.kind === 'only_in_new'} class:bad={e.kind === 'only_in_old'} class:warn={e.kind === 'changed'}>{e.kind}</span></td>
                <td class="mono is-dim">{e.parser_before ?? '—'}</td>
                <td class="mono">{e.parser_after ?? '—'}</td>
                <td class="mono is-ok">{pairs(e.added).map(([k, v]) => `${k}=${v}`).join('  ')}</td>
                <td class="mono is-bad">{pairs(e.lost).map(([k, v]) => `${k}=${v}`).join('  ')}</td>
                <td class="mono is-warn">{pairs(e.changed).map(([k, v]) => `${k}: ${v[0]} to ${v[1]}`).join('  ')}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>
{/if}
