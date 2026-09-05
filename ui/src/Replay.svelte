<script>
  import { api, fmt } from './api.js'
  import { live } from './state.svelte.js'
  import { keys, nav } from './keys.js'
  import VList from './VList.svelte'
  import Confirm from './Confirm.svelte'

  let data = $state(null)
  let err = $state(null)
  let busy = $state(false)
  let asking = $state(false)
  let started = $state(null)
  let entries = $state([])
  let kindFilter = $state('')
  let nextAfter = $state(null)
  let diffVersion = $state(null)
  let diffErr = $state(null)
  let sel = $state(-1)
  let innerHeight = $state(800)

  async function load() {
    const r = await api('GET', '/api/replay')
    if (r.ok) { data = r.data; err = null } else err = r.data
  }
  load()
  // The SSE replay frame drives progress; every state change re-reads the versions list.
  $effect(() => { live.replay?.state; load() })

  const running = $derived(live.replay?.state === 'progress' || live.replay?.state === 'started' ? live.replay : data?.running)
  const report = $derived(data?.last ?? (live.replay?.report ?? null))
  const canStart = $derived(!busy && !running && data && (live.integrity?.records ?? 1) > 0)

  async function start() {
    asking = false
    busy = true
    const r = await api('POST', '/api/replay', {})
    busy = false
    if (r.ok) { started = r.data; load() }
    else err = r.data
  }

  async function loadDiff(version, after = null) {
    diffVersion = version
    diffErr = null
    const u = `/api/replay/${version}/diff?limit=500${after != null ? `&after=${after}` : ''}${kindFilter ? `&kind=${kindFilter}` : ''}`
    const r = await api('GET', u)
    if (!r.ok) { diffErr = r.data; entries = []; nextAfter = null; return }
    entries = after == null ? r.data.entries : [...entries, ...r.data.entries]
    nextAfter = r.data.next_after
  }
  $effect(() => { kindFilter; if (diffVersion) loadDiff(diffVersion, null) })
  // The newest report's diff opens by itself: the entries are what the counters summarise.
  $effect(() => { if (report?.diff && diffVersion == null) loadDiff(report.version, null) })
  $effect(() => keys((e) => {
    if (asking) return false
    if (e.key === 'v' && canStart) { asking = true; return true }
    if (e.key === 'm' && nextAfter != null) { loadDiff(diffVersion, nextAfter); return true }
    return nav(e, entries.length, sel, (n) => (sel = n), (n) => (location.hash = `#/trace/${entries[n].raw_id}`))
  }))

  const pairs = (o) => Object.entries(o ?? {})
  const base = (p) => String(p ?? '').split('/').pop()
  const describe = (e) => [
    ...pairs(e.added).map(([k, v]) => ({ t: 'add', s: `+${k}=${v}` })),
    ...pairs(e.lost).map(([k, v]) => ({ t: 'del', s: `-${k}=${v}` })),
    ...pairs(e.changed).map(([k, v]) => ({ t: 'chg', s: `${k}: ${v[0]} to ${v[1]}` })),
  ]
</script>

<svelte:window bind:innerHeight />

<section>
  <div class="head">
    <h2>Replay</h2>
    <span class="note">the same stored bytes through today's parsers, versioned beside the live output; the store is only read</span>
    <span class="push bar">
      {#if running}<span class="tag warn">v{running.version} running</span>{/if}
      <button class="btn primary" onclick={() => (asking = true)} disabled={!canStart}>Replay the store<kbd>v</kbd></button>
    </span>
  </div>

  {#if asking}
    <Confirm title="Replay every stored record through the current parsers?" verb="Replay" onconfirm={start} oncancel={() => (asking = false)}
             hint="The live output is version 1 and is never touched. The store is read through a snapshot; the engine keeps ingesting. A parser approved mid-replay takes effect for the live stream and the next replay.">
      <span>writes <code>{base(live.status?.output ?? 'out.jsonl').replace(/\.jsonl$/, '')}.v{(data?.versions?.length ?? 1) + 1}.jsonl</code> beside the output, with its meta and a diff against the previous version</span>
    </Confirm>
  {/if}
  {#if err}<div class="notice bad"><b>{err.error}</b><span class="muted">{err.reason}</span></div>{/if}
  {#if data?.last_error}<div class="notice bad"><b>The last replay failed</b><span class="mono">{data.last_error}</span></div>{/if}
  {#if started && !running && report?.version !== started.version}<div class="notice ok"><b>Started version {started.version} over {fmt.n(started.total)} records.</b></div>{/if}

  {#if running}
    <div class="panel pad stack">
      <div class="bar sm"><b class="mono">v{running.version}</b><span class="muted">{fmt.n(running.done)} of {fmt.n(running.total)} records</span><span class="push mono muted">{running.total ? Math.floor((100 * running.done) / running.total) : 0}%</span></div>
      <div class="meter"><i style="width:{running.total ? (100 * running.done) / running.total : 0}%"></i></div>
    </div>
  {/if}
</section>

{#if report}
  <section class="stack">
    <div class="why">
      <div class="head quiet"><h3>Why v{report.version} differs from v{report.previous_version ?? '–'}</h3><span class="note">{fmt.n(report.why?.length ?? 0)} line{report.why?.length === 1 ? '' : 's'} from the report, verbatim</span></div>
      <div class="lines">
        {#each report.why ?? [] as w}<p>{w}</p>{:else}<p class="is-dim">The report carries no explanation.</p>{/each}
      </div>
      <div class="paths">
        <span>{fmt.n(report.events)} events in {fmt.f(report.elapsed_secs, 2)}s, {fmt.n(Math.round(report.events_per_sec))} per second, parsers generation {report.parsers_generation}</span>
        <span>output <span class="mono" title={report.output}>{base(report.output)}</span></span>
        {#if report.diff}<span>diff <span class="mono" title={report.diff}>{base(report.diff)}</span></span>{/if}
      </div>
    </div>

    <div class="counters">
      <b>events</b>
      <span class="kvs">
        <span class="kv"><span>unchanged</span><span class="num">{fmt.n(report.summary?.unchanged)}</span></span>
        <span class="kv" class:on={report.summary?.changed > 0}><span>changed</span><span class="num">{fmt.n(report.summary?.changed)}</span></span>
        <span class="kv" class:ok={report.summary?.only_in_new > 0}><span>only in new</span><span class="num">{fmt.n(report.summary?.only_in_new)}</span></span>
        <span class="kv" class:bad={report.summary?.only_in_old > 0}><span>only in old</span><span class="num">{fmt.n(report.summary?.only_in_old)}</span></span>
      </span>
      <b>fields</b>
      <span class="kvs">
        <span class="kv" class:ok={report.summary?.fields_added > 0}><span>added</span><span class="num">{fmt.n(report.summary?.fields_added)}</span></span>
        <span class="kv" class:bad={report.summary?.fields_lost > 0}><span>lost</span><span class="num">{fmt.n(report.summary?.fields_lost)}</span></span>
        <span class="kv" class:on={report.summary?.fields_changed > 0}><span>changed</span><span class="num">{fmt.n(report.summary?.fields_changed)}</span></span>
      </span>
    </div>

    <div class="two">
      <div>
        <div class="head"><h2>Parser changes</h2><span class="note">which parser claimed an event before and after</span></div>
        <div class="scroll" style="max-height:34vh">
        <table class="tbl">
          <thead><tr><th>before</th><th>after</th><th class="num">events</th></tr></thead>
          <tbody>
            {#each report.summary?.parser_changes ?? [] as c}
              <tr>
                <td class="mono">{#if c.from}{c.from}{:else}<span class="tag warn">no parser</span>{/if}</td>
                <td class="mono">{#if c.to}{c.to}{:else}<span class="tag warn">no parser</span>{/if}</td>
                <td class="num">{fmt.n(c.events)}</td>
              </tr>
            {:else}
              <tr><td colspan="3" class="is-dim">No event changed the parser that claimed it.</td></tr>
            {/each}
          </tbody>
        </table>
        </div>
      </div>
      <div>
        <div class="head"><h2>By field</h2><span class="note">{fmt.n(report.summary?.by_field?.length ?? 0)} schema paths, most affected first</span></div>
        <div class="scroll" style="max-height:34vh">
        <table class="tbl">
          <thead><tr><th>path</th><th class="num">added</th><th class="num">lost</th><th class="num">changed</th></tr></thead>
          <tbody>
            {#each report.summary?.by_field ?? [] as f}
              <tr>
                <td class="mono">{f.path}</td>
                <td class="num" class:is-ok={f.added > 0} class:is-dim={!f.added}>{fmt.n(f.added)}</td>
                <td class="num" class:is-bad={f.lost > 0} class:is-dim={!f.lost}>{fmt.n(f.lost)}</td>
                <td class="num" class:is-warn={f.changed > 0} class:is-dim={!f.changed}>{fmt.n(f.changed)}</td>
              </tr>
            {:else}
              <tr><td colspan="4" class="is-dim">No schema field gained, lost or changed a value.</td></tr>
            {/each}
          </tbody>
        </table>
        </div>
      </div>
    </div>
  </section>
{/if}

<section>
  <div class="head"><h2>Versions</h2><span class="note">version 1 is the live output; every replay adds one beside it</span></div>
  {#if !data}
    <p class="loading">reading the output versions</p>
  {:else if !data.versions?.length}
    <div class="empty"><b>No output versions yet.</b><span>Version 1 appears with the first emitted event.</span></div>
  {:else}
    <div class="wrap"><table class="tbl">
      <thead><tr><th>version</th><th>file</th><th>created</th><th class="num">events</th><th>schema</th><th class="num">parsers gen</th><th>diff</th><th class="fill"></th></tr></thead>
      <tbody>
        {#each data.versions as v (v.version)}
          <tr>
            <td class="mono">v{v.version}</td>
            <td class="mono is-dim" title={v.path}>{base(v.path)}</td>
            <td class="mono is-dim">{fmt.stamp(v.created)}</td>
            <td class="num">{fmt.n(v.events)}</td>
            <td>{v.schema}</td>
            <td class="num">{fmt.n(v.parsers_generation)}</td>
            <td>{#if v.version > 1}<button class="btn" class:on={diffVersion === v.version} onclick={() => loadDiff(v.version, null)}>{diffVersion === v.version ? 'Showing' : 'Open diff'}</button>{:else}<span class="is-dim">baseline</span>{/if}</td>
            <td class="fill"></td>
          </tr>
        {/each}
      </tbody>
    </table></div>
  {/if}
</section>

{#if diffVersion}
  <section>
    <div class="head">
      <h2>Diff entries, v{diffVersion} against v{diffVersion - 1}</h2>
      <span class="note">{fmt.n(entries.length)} loaded{nextAfter != null ? ', more below' : ''}, Enter traces the selected event</span>
      <span class="push bar">
        <span class="kinds" role="radiogroup" aria-label="Entry kind">
          {#each [['', 'all'], ['changed', 'changed'], ['only_in_new', 'only in new'], ['only_in_old', 'only in old']] as [k, label]}
            <button class:on={kindFilter === k} onclick={() => (kindFilter = k)} role="radio" aria-checked={kindFilter === k}>{label}</button>
          {/each}
        </span>
        {#if nextAfter != null}<button class="btn" onclick={() => loadDiff(diffVersion, nextAfter)}>Load more<kbd>m</kbd></button>{/if}
      </span>
    </div>
    {#if diffErr}
      <div class="notice bad"><b>{diffErr.error}</b><span class="muted">{diffErr.reason}</span></div>
    {:else if !entries.length}
      <div class="empty"><b>No entries of this kind.</b><span>{kindFilter ? 'Pick another kind above.' : 'Every event in this version is identical to the one before.'}</span></div>
    {:else}
      <div style="--cols:7em 8em 12em 12em minmax(0,1fr)">
        <VList items={entries} max={Math.max(396, innerHeight - 300)} {sel}>
          {#snippet header()}<div class="vh"><span class="num">raw</span><span>kind</span><span>parser before</span><span>parser after</span><span>fields</span></div>{/snippet}
          {#snippet row(e, i)}
            <div class="vr" class:sel={i === sel} onclick={() => (location.hash = `#/trace/${e.raw_id}`)} role="button" tabindex="-1">
              <span class="num">{e.raw_id}</span>
              <span><span class="tag" class:ok={e.kind === 'only_in_new'} class:bad={e.kind === 'only_in_old'} class:warn={e.kind === 'changed'}>{e.kind.replaceAll('_', ' ')}</span></span>
              <span class="mono is-dim">{e.parser_before ?? 'none'}</span>
              <span class="mono">{e.parser_after ?? 'none'}</span>
              <span class="mono fields">{#each describe(e) as d}<span class={d.t} title={d.s}>{d.s}</span>{/each}</span>
            </div>
          {/snippet}
        </VList>
      </div>
    {/if}
  </section>
{/if}
