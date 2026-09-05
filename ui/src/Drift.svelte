<script>
  import { api, fmt } from './api.js'
  import { live } from './state.svelte.js'
  import { keys, nav } from './keys.js'

  let list = $state(null)
  let err = $state(null)
  let sel = $state(-1)

  // The state of each source when this screen opened; a state that changes later pops, the initial one does not.
  let seen = null
  async function load() {
    const r = await api('GET', '/api/drift')
    if (r.ok) { list = r.data; err = null; seen ??= Object.fromEntries(list.map((d) => [d.source, d.state])) } else err = r.data
  }
  load()
  $effect(() => { live.drift.length; load() })

  const order = { tripped: 0, proposed: 1, watching: 2, cleared: 3 }
  // The alert carries its own pending id once inference has run; until then the source row
  // in MetricsFrame is the one that knows, so a tripped source still links to its proposal.
  const proposalOf = (d) =>
    d.pending_id ?? (live.metrics?.sources ?? []).find((s) => s.name === d.source)?.pending_id ?? null
  const rows = $derived([...(list ?? [])].sort((a, b) => (order[a.state] ?? 9) - (order[b.state] ?? 9)))
  const eng = $derived(live.metrics?.engine ?? {})
  const excess = (d) => (d.window?.rate ?? 0) - (d.baseline_rate ?? 0)
  const tone = (s) => (s === 'tripped' ? 'warn' : s === 'proposed' ? 'pend' : s === 'cleared' ? 'ok' : '')
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
    <div class="counters" style="margin-bottom:var(--s5)">
      <b>engine</b>
      <span class="kvs">
        <span class="kv" class:on={eng.drift_tripped > 0}><span>sources tripped</span><span class="num">{fmt.n(eng.drift_tripped)}</span></span>
        <span class="kv"><span>lines routed to inference</span><span class="num">{fmt.n(eng.drift_lines_routed)}</span></span>
        <span class="kv" class:pend={eng.drift_proposals > 0}><span>update proposals</span><span class="num">{fmt.n(eng.drift_proposals)}</span></span>
        <span class="kv" class:ok={eng.drift_cleared > 0}><span>cleared</span><span class="num">{fmt.n(eng.drift_cleared)}</span></span>
      </span>
    </div>
  {/if}
  {#if err}
    <div class="notice bad"><b>{err.error}</b><span class="muted">{err.reason}</span></div>
  {:else if !list}
    <p class="loading">reading the drift table</p>
  {:else if !rows.length}
    <div class="empty">
      <b>No source is established yet.</b>
      <span>A source is watched after 1,024 events with a long-run miss rate under 20%. It trips when a 512-event window misses 0.25 above that baseline with at least 32 misses, and its misses go to inference with the current parser as the prior.</span>
    </div>
  {:else}
    <div class="wrap"><table class="tbl">
      <thead>
        <tr><th>state</th><th>source</th><th>parser</th><th class="num">window misses</th><th class="num">window rate</th><th class="num">baseline</th><th class="num">excess</th><th class="num">routed</th><th>since</th><th>proposal</th><th class="fill"></th></tr>
      </thead>
      <tbody>
        {#each rows as d, i (d.source)}
          <tr class:sel={i === sel} class:click={!!proposalOf(d)} onclick={() => proposalOf(d) && (location.hash = `#/review/${encodeURIComponent(proposalOf(d))}`)}>
            <td>{#key d.state}<span class="tag {tone(d.state)}" class:pop={seen && seen[d.source] !== d.state}>{d.state}</span>{/key}</td>
            <td class="mono">{d.source}</td>
            <td class="mono">{d.parser}</td>
            <td class="num" title={d.window?.events ? '' : 'the window was drained when the source tripped; its misses went to inference'}>
              {#if d.window?.events}{fmt.n(d.window.misses)} / {fmt.n(d.window.events)}{:else}<span class="is-dim">drained</span>{/if}
            </td>
            <td class="num" class:is-warn={d.state === 'tripped' || d.state === 'proposed'}>{fmt.pct(d.window?.rate)}</td>
            <td class="num is-dim">{fmt.pct(d.baseline_rate)}</td>
            <td class="num" class:is-warn={excess(d) >= 0.25}>{excess(d) >= 0 ? '+' : ''}{fmt.pct(excess(d))}</td>
            <td class="num">{fmt.n(d.lines_routed)}</td>
            <td class="mono is-dim">{fmt.stamp(d.since)}</td>
            <td>
              {#if proposalOf(d)}
                <a class="mono" href="#/review/{encodeURIComponent(proposalOf(d))}">{d.proposed_version ? `${proposalOf(d)} v${d.proposed_version}` : proposalOf(d)}</a>
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
    <p class="sm muted" style="margin-top:var(--s4)">Enter on a tripped or proposed row opens its update proposal in Review; the proposal is the established parser plus the new templates, with a diff against the file on disk.</p>
  {/if}
</section>
