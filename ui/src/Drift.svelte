<script>
  import { api, fmt } from './api.js'
  import { live } from './state.svelte.js'
  import { keys, nav } from './keys.js'

  let list = $state([])
  let err = $state(null)
  let sel = $state(-1)

  async function load() {
    const r = await api('GET', '/api/drift')
    if (r.ok) { list = r.data; err = null } else err = r.data
  }
  load()
  $effect(() => { live.drift.length; load() })

  const order = { tripped: 0, proposed: 1, watching: 2, cleared: 3 }
  // The alert carries its own pending id once inference has run; until then the source row
  // in MetricsFrame is the one that knows, so a tripped source still links to its proposal.
  const proposalOf = (d) =>
    d.pending_id ?? (live.metrics?.sources ?? []).find((s) => s.name === d.source)?.pending_id ?? null
  const rows = $derived([...list].sort((a, b) => (order[a.state] ?? 9) - (order[b.state] ?? 9)))
  const eng = $derived(live.metrics?.engine ?? {})
  $effect(() => keys((ev) => nav(ev, rows.length, sel, (n) => (sel = n), (n) => {
    const p = proposalOf(rows[n])
    if (p) location.hash = `#/review/${encodeURIComponent(p)}`
  })))
</script>

<section>
  <div class="head">
    <h2>Drift</h2>
    <span class="note">a source whose established parser started missing: the window's miss rate against the long run</span>
  </div>
  {#if eng.drift_tripped != null}
    <div class="counters" style="margin-bottom:var(--s3)">
      <div class="crow">
        <b>engine</b>
        <span class="kvs">
          <span class="kv" class:on={eng.drift_tripped > 0}><span>sources tripped</span><span class="num">{fmt.n(eng.drift_tripped)}</span></span>
          <span class="kv"><span>lines routed to inference</span><span class="num">{fmt.n(eng.drift_lines_routed)}</span></span>
          <span class="kv" class:on={eng.drift_proposals > 0}><span>update proposals</span><span class="num">{fmt.n(eng.drift_proposals)}</span></span>
          <span class="kv" class:ok={eng.drift_cleared > 0}><span>cleared</span><span class="num">{fmt.n(eng.drift_cleared)}</span></span>
        </span>
      </div>
    </div>
  {/if}
  {#if err}
    <p class="notice bad">{err.error} ({err.reason})</p>
  {:else if !rows.length}
    <p class="empty">No source is established yet. A source becomes watched after 1,024 events with a long-run miss rate under 20%.</p>
  {:else}
    <div class="wrap"><table class="tbl">
      <thead>
        <tr><th>state</th><th>source</th><th>parser</th><th class="num">window misses</th><th class="num">window rate</th><th class="num">baseline</th><th class="num">excess</th><th class="num">routed</th><th>since</th><th>proposal</th><th class="fill"></th></tr>
      </thead>
      <tbody>
        {#each rows as d, i (d.source)}
          <tr class:sel={i === sel} class:click={!!proposalOf(d)} onclick={() => proposalOf(d) && (location.hash = `#/review/${encodeURIComponent(proposalOf(d))}`)}>
            <td>
              {#if d.state === 'tripped'}<span class="tag warn">tripped</span>
              {:else if d.state === 'proposed'}<span class="tag accent">proposed</span>
              {:else if d.state === 'cleared'}<span class="tag ok">cleared</span>
              {:else}<span class="tag">watching</span>{/if}
            </td>
            <td class="mono">{d.source}</td>
            <td class="mono">{d.parser}</td>
            <td class="num" title={d.window?.events ? '' : 'the window was drained when the source tripped; its misses went to inference'}>
              {#if d.window?.events}{fmt.n(d.window.misses)} / {fmt.n(d.window.events)}{:else}<span class="is-dim">drained</span>{/if}
            </td>
            <td class="num" class:is-warn={d.state === 'tripped' || d.state === 'proposed'}>{fmt.pct(d.window?.rate)}</td>
            <td class="num is-dim">{fmt.pct(d.baseline_rate)}</td>
            <td class="num" class:is-warn={(d.window?.rate ?? 0) - (d.baseline_rate ?? 0) >= 0.25}>{fmt.pct((d.window?.rate ?? 0) - (d.baseline_rate ?? 0))}</td>
            <td class="num">{fmt.n(d.lines_routed)}</td>
            <td class="mono is-dim">{d.since || '–'}</td>
            <td>
              {#if proposalOf(d)}
                <a href="#/review/{encodeURIComponent(proposalOf(d))}">{d.proposed_version ? `${proposalOf(d)} v${d.proposed_version}` : proposalOf(d)}</a>
              {:else if d.state === 'tripped'}
                <span class="is-dim">inference is still building one</span>
              {:else}
                <span class="is-dim">none</span>
              {/if}
            </td>
            <td class="fill"></td>
          </tr>
        {/each}
      </tbody>
    </table></div>
    <p class="sm muted">A source trips when the window's miss rate exceeds the long-run rate by 0.25 or more with at least 32 misses in the window. Its misses are then routed to inference with the established parser as the prior, and the proposal is that parser plus the new templates.</p>
  {/if}
</section>
