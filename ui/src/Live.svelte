<script>
  import { live, resume } from './state.svelte.js'
  import { fmt } from './api.js'
  import { keys, nav } from './keys.js'

  const m = $derived(live.metrics)
  const e = $derived(live.metrics?.engine ?? {})
  const idle = $derived(!live.metrics || !live.metrics.engine?.framed)
  const alerts = $derived([
    ...(live.drift ?? []).filter((d) => d.state === 'tripped' || d.state === 'proposed').map((d) => ({
      tone: d.state === 'tripped' ? 'warn' : 'accent',
      text: d.window?.events
        ? `${d.source} drifting from ${d.parser}: ${fmt.n(d.window.misses)} of the last ${fmt.n(d.window.events)} events miss, baseline ${fmt.pct(d.baseline_rate)}`
        : `${d.source} drifting from ${d.parser}: ${fmt.pct(d.window.rate)} of its window missed, baseline ${fmt.pct(d.baseline_rate)}`,
      href: d.pending_id ? `#/review/${encodeURIComponent(d.pending_id)}` : '#/drift',
      link: d.pending_id ? 'review the update' : 'drift',
    })),
    ...(live.pending.count ? [{ tone: 'accent', text: `${live.pending.count} proposal${live.pending.count === 1 ? '' : 's'} waiting for a human`, href: '#/review', link: 'review' }] : []),
    ...(live.integrity?.last_verify && !live.integrity.last_verify.ok
      ? [{ tone: 'bad', text: `integrity broken at raw id ${live.integrity.last_verify.first_bad} (${live.integrity.last_verify.reason})`, href: '#/integrity', link: 'integrity' }]
      : []),
  ])

  // The funnel: every stage of the pipeline and what fell out between two of them.
  const failed = $derived((e.parse_failed ?? []).reduce((a, [, n]) => a + n, 0))
  const funnel = $derived([
    { k: 'framed', n: e.framed, lost: 0, label: '', why: '' },
    { k: 'stored', n: e.stored, lost: 0, label: '', why: '' },
    { k: 'detected', n: e.detected, lost: e.no_parser ?? 0, label: 'no parser', why: 'no parser claimed the format; these lines feed inference' },
    { k: 'parsed', n: e.parsed, lost: failed, label: 'parse failed', why: `parse failed: ${fmt.pairs(e.parse_failed)}` },
    { k: 'normalized', n: e.normalized, lost: 0, label: '', why: '' },
    { k: 'emitted', n: e.emitted, lost: 0, label: '', why: '' },
  ])

  let sel = $state(-1)
  let filter = $state('')
  let box = $state(null)
  const match = (r) => {
    const q = filter.trim().toLowerCase()
    return !q || `${r.raw_id} ${r.parser ?? r.status} ${r.cls} ${r.action} ${r.device} ${r.sum}`.toLowerCase().includes(q)
  }
  const rows = $derived(filter.trim() ? live.tail.filter(match) : live.tail)
  $effect(() => { filter; sel = -1 })
  $effect(() => keys((ev) => {
    if (ev.key === '/') { box?.focus(); box?.select(); return true }
    if (ev.key === ' ') { live.paused ? resume() : (live.paused = true); return true }
    const was = sel
    const hit = nav(ev, rows.length, sel, (n) => (sel = n), (n) => (location.hash = `#/trace/${rows[n].raw_id}`))
    if (hit && sel !== was && !live.paused) live.paused = true // reading a row holds the tail still
    return hit
  }))
</script>

<section class="hero">
  <div class="rates">
    <div class="rate"><b class="num">{fmt.f(e.events_per_sec, 0)}</b><span>events per second</span></div>
    <div class="rate"><b class="num">{fmt.f(e.mb_per_sec, 1)}</b><span>MB per second</span></div>
    {#if idle}<span class="tag">idle, waiting for input</span>{/if}
    {#if e.backpressure_blocks > 0}<span class="tag warn">producer blocked {fmt.n(e.backpressure_blocks)}×</span>{/if}
  </div>
  <div class="funnel">
    {#each funnel as f}
      <div class="fst">
        <span class="num">{fmt.n(f.n)}</span>
        <span class="lab">{f.k}</span>
        <span class="track"><i style="width:{e.framed ? (100 * f.n) / e.framed : 0}%"></i></span>
        {#if f.lost > 0}<span class="loss" title={f.why}>−{fmt.n(f.lost)} {f.label}</span>{:else}<span class="loss"></span>{/if}
      </div>
    {/each}
  </div>
</section>

<section>
  <div class="head"><h2>Engine</h2><span class="note">every counter the run block prints, live</span></div>
  <div class="counters">
    <div class="crow">
      <b>input</b>
      <span class="kvs">
        <span class="kv"><span>files</span><span class="num">{fmt.n(e.files)}</span></span>
        <span class="kv" class:bad={e.files_failed > 0}><span>failed</span><span class="num">{fmt.n(e.files_failed)}</span></span>
        <span class="kv"><span>MB in</span><span class="num">{fmt.mb(e.bytes)}</span></span>
        <span class="kv"><span>output bytes</span><span class="num">{fmt.n(e.output_bytes)}</span></span>
        <span class="kv"><span>elapsed</span><span class="num">{fmt.f(e.elapsed_secs, 1)}s</span></span>
        <span class="kv"><span>threads</span><span class="num">{fmt.n(e.threads)}</span></span>
        <span class="kv" class:bad={e.parse_failed?.length}><span>parse_failed</span><span class="num">{fmt.pairs(e.parse_failed)}</span></span>
      </span>
    </div>
    <div class="crow">
      <b>signals</b>
      <span class="kvs">
        {#each ['sub_matched', 'sub_no_match', 'sub_uncovered', 'time_from_receipt', 'class_unknown', 'enum_other', 'unmapped_fields', 'utf8_lossy'] as k}
          <span class="kv" class:on={e[k] > 0 && k !== 'sub_matched'}><span>{k}</span><span class="num">{fmt.n(e[k])}</span></span>
        {/each}
        <span class="kv" class:on={e.time_error?.length}><span>time_error</span><span class="num">{fmt.pairs(e.time_error)}</span></span>
      </span>
    </div>
    <div class="crow">
      <b>queue</b>
      <span class="kvs">
        <span class="kv"><span>batches</span><span class="num">{fmt.n(e.batches)}</span></span>
        <span class="kv"><span>high-water</span><span class="num">{fmt.n(e.queue_high_water)}/{fmt.n(e.queue_capacity)}</span></span>
        <span class="kv" class:on={e.backpressure_blocks > 0}><span>backpressure blocks</span><span class="num">{fmt.n(e.backpressure_blocks)}</span></span>
      </span>
    </div>
    <div class="crow">
      <b>inference</b>
      <span class="kvs">
        {#each ['infer_buffered', 'infer_buffer_full', 'infer_runs', 'infer_lines_templated', 'infer_lines_unmatched', 'proposals_written', 'proposals_replaced', 'approved', 'rejected', 'reloads'] as k}
          <span class="kv" class:ok={k === 'approved' && e[k] > 0}><span>{k}</span><span class="num">{fmt.n(e[k])}</span></span>
        {/each}
        <span class="kv"><span>skipped</span><span class="num">{fmt.pairs(e.proposals_skipped)}</span></span>
      </span>
    </div>
    {#if e.drift_tripped != null || e.syslog_udp_datagrams != null}
      <div class="crow">
        <b>drift</b>
        <span class="kvs">
          {#each ['drift_tripped', 'drift_lines_routed', 'drift_proposals', 'drift_cleared'] as k}
            <span class="kv" class:on={k === 'drift_tripped' && e[k] > 0}><span>{k.replace('drift_', '')}</span><span class="num">{fmt.n(e[k])}</span></span>
          {/each}
        </span>
      </div>
    {/if}
    {#if m?.syslog || e.syslog_udp_datagrams != null}
      <div class="crow">
        <b>syslog</b>
        <span class="kvs">
          <span class="kv"><span>udp datagrams</span><span class="num">{fmt.n(m?.syslog?.udp_datagrams ?? e.syslog_udp_datagrams)}</span></span>
          <span class="kv"><span>udp bytes</span><span class="num">{fmt.n(e.syslog_udp_bytes)}</span></span>
          <span class="kv"><span>tcp connections</span><span class="num">{fmt.n(m?.syslog?.tcp_connections ?? e.syslog_tcp_connections)}</span></span>
          <span class="kv"><span>tcp events</span><span class="num">{fmt.n(m?.syslog?.tcp_events ?? e.syslog_tcp_events)}</span></span>
          <span class="kv" class:on={e.syslog_tcp_partial > 0}><span>tcp partial</span><span class="num">{fmt.n(e.syslog_tcp_partial)}</span></span>
        </span>
      </div>
    {/if}
    {#if live.integrity}
      <div class="crow">
        <b>integrity</b>
        <span class="kvs">
          <span class="kv"><span>records</span><span class="num">{fmt.n(live.integrity.records)}</span></span>
          <span class="kv"><span>head</span><span class="num">{fmt.hex(live.integrity.head)}</span></span>
          {#if live.integrity.running}
            <span class="kv on"><span>verify</span><span class="num">running</span></span>
          {:else if live.integrity.last_verify}
            <span class="kv" class:ok={live.integrity.last_verify.ok} class:bad={!live.integrity.last_verify.ok}>
              <span>last verify</span><span class="num">{live.integrity.last_verify.ok ? 'clean' : `first bad ${live.integrity.last_verify.first_bad}`}</span>
            </span>
            <span class="kv"><span>at</span><span class="num">{live.integrity.last_verify.at}</span></span>
          {:else}
            <span class="kv"><span>last verify</span><span class="num is-dim">never</span></span>
          {/if}
        </span>
      </div>
    {/if}
    {#if m?.pivot}
      <div class="crow">
        <b>pivot</b>
        <span class="kvs">
          <span class="kv"><span>postings</span><span class="num">{fmt.n(m.pivot.postings)}</span></span>
          <span class="kv"><span>batches</span><span class="num">{fmt.n(m.pivot.batches)}</span></span>
          <span class="kv" class:on={m.pivot.blocked > 0}><span>blocked</span><span class="num">{fmt.n(m.pivot.blocked)}</span></span>
          <span class="kv" class:bad={m.pivot.errors > 0}><span>errors</span><span class="num">{fmt.n(m.pivot.errors)}</span></span>
          <span class="kv"><a class="sm" href="#/pivot">open pivot</a></span>
        </span>
      </div>
    {/if}
    {#if m?.replay}
      <div class="crow">
        <b>replay</b>
        <span class="kvs">
          <span class="kv"><span>latest version</span><span class="num">{m.replay.last_version == null ? 'none yet' : `v${fmt.n(m.replay.last_version)}`}</span></span>
          <span class="kv" class:on={m.replay.running}><span>state</span><span class="num">{m.replay.running ? 'running' : 'idle'}</span></span>
          <span class="kv"><a class="sm" href="#/replay">open replay</a></span>
        </span>
      </div>
    {/if}
  </div>
</section>

{#if alerts.length}
  <section class="alerts">
    {#each alerts as a}
      <div class="alert {a.tone}"><b>{a.text}</b><a href={a.href}>{a.link}</a></div>
    {/each}
  </section>
{/if}

<div class="split sources">
  <section>
    <div class="head"><h2>Sources</h2><span class="note">{fmt.n(m?.sources?.length ?? 0)} seen this run</span></div>
    {#if !m?.sources?.length}
      <p class="empty">No sources yet. They appear when the first file or watched directory is read.</p>
    {:else}
      <div class="wrap">
        <table class="tbl">
          <thead><tr><th>source</th><th>parser</th><th class="num">events</th><th class="num">detected</th><th class="num">no_parser</th><th class="num">failed</th><th class="num">buffered</th><th class="num">window</th><th class="num">baseline</th><th>drift</th><th>proposal</th><th class="fill"></th></tr></thead>
          <tbody>
            {#each m.sources as s (s.name)}
              <tr>
                <td class="mono">{s.name}</td>
                <td class="mono">{#if s.parser}{s.parser}{:else if 'parser' in s}<span class="is-dim">none</span>{/if}</td>
                <td class="num">{fmt.n(s.events)}</td>
                <td class="num">{fmt.n(s.detected)}</td>
                <td class="num" class:is-warn={s.no_parser > 0}>{fmt.n(s.no_parser)}</td>
                <td class="num" class:is-bad={s.parse_failed > 0}>{s.parse_failed == null ? '' : fmt.n(s.parse_failed)}</td>
                <td class="num">{fmt.n(s.buffered)}</td>
                <td class="num">{s.window_rate == null ? '' : fmt.pct(s.window_rate)}</td>
                <td class="num">{s.baseline_rate == null ? '' : fmt.pct(s.baseline_rate)}</td>
                <td>
                  {#if s.drift === 'tripped'}<span class="tag warn">tripped</span>
                  {:else if s.drift === 'proposed'}<span class="tag accent">proposed</span>
                  {:else if s.drift === 'watching'}<span class="tag">watching</span>
                  {:else if s.drift}<span class="is-dim">–</span>{/if}
                </td>
                <td>{#if s.pending_id}<a class="mono" href="#/review/{encodeURIComponent(s.pending_id)}">{s.pending_id}</a>{:else}<span class="is-dim">–</span>{/if}</td>
                <td class="fill"></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>
  <section>
    <div class="head"><h2>Parsers</h2><span class="note">{fmt.n(m?.parsers?.length ?? 0)} loaded</span></div>
    {#if !m?.parsers?.length}
      <p class="empty">No parsers loaded.</p>
    {:else}
      <div class="wrap">
        <table class="tbl">
          <thead><tr><th>name</th><th>device</th><th>strategy</th><th class="num">subs</th><th class="num">prio</th><th>origin</th><th class="num">detected</th><th class="fill"></th></tr></thead>
          <tbody>
            {#each m.parsers as p (p.name)}
              <tr>
                <td class="mono">{p.name}</td>
                <td class="tight" title="{p.vendor} {p.product}">{p.vendor} {p.product}</td>
                <td class="mono is-dim">{p.strategy}</td>
                <td class="num">{fmt.n(p.subs)}</td>
                <td class="num">{fmt.n(p.priority)}</td>
                <td>{#if p.origin === 'approved'}<span class="tag ok">approved</span>{:else}<span class="is-dim">hand</span>{/if}</td>
                <td class="num">{fmt.n(p.detected)}</td>
                <td class="fill"></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>
</div>

<section>
  <div class="head">
    <h2>Tail</h2>
    <span class="note">newest first, {filter.trim() ? `${fmt.n(rows.length)} of ${fmt.n(live.tail.length)}` : `${fmt.n(rows.length)}`} rows</span>
    <span class="push bar">
      <input type="search" bind:value={filter} bind:this={box} onkeydown={(ev) => { if (ev.key === 'Escape') { filter = ''; ev.currentTarget.blur() } }} placeholder="filter the tail  /" size="24" aria-label="Filter the tail" />
      {#if live.paused}<span class="tag warn">held, {fmt.n(live.held)} arrived</span>{/if}
      <button class="btn" class:on={live.paused} onclick={() => (live.paused ? resume() : (live.paused = true))}>{live.paused ? 'Release' : 'Hold'}</button>
    </span>
  </div>
  {#if !rows.length}
    <p class="empty">{filter.trim() ? `Nothing in the tail matches ${filter.trim()}.` : 'No events yet. The tail fills as the engine emits.'}</p>
  {:else}
    <div class="scroll">
      <table class="tbl fixed">
        <colgroup><col style="width:5.5em" /><col style="width:12em" /><col style="width:12em" /><col style="width:11em" /><col style="width:7em" /><col style="width:14em" /><col /></colgroup>
        <thead><tr><th class="num">raw</th><th>time</th><th>parser</th><th>class</th><th>action</th><th>device</th><th>summary</th></tr></thead>
        <tbody>
          {#each rows as r, i (r.raw_id)}
            <tr class="click" class:sel={i === sel} onclick={() => (location.hash = `#/trace/${r.raw_id}`)}>
              <td class="num">{r.raw_id}</td>
              <td class="mono is-dim">{r.time}</td>
              <td class="mono">{#if r.parser}{r.parser}{:else}<span class="is-warn">{r.status}</span>{/if}</td>
              <td>{r.cls}</td>
              <td class:is-warn={r.action === 'Denied' || r.action === 'Blocked'}>{r.action}</td>
              <td class="mono is-dim">{r.device}</td>
              <td class="mono" title={r.sum}>{fmt.cut(r.sum, 200)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>
