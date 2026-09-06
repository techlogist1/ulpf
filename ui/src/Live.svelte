<script>
  import { live, resume } from './state.svelte.js'
  import { fmt } from './api.js'
  import { keys, nav } from './keys.js'
  import VList from './VList.svelte'
  import Flags from './Flags.svelte'

  const m = $derived(live.metrics)
  const e = $derived(live.metrics?.engine ?? {})
  const idle = $derived(!live.metrics || !live.metrics.engine?.framed)
  const alerts = $derived([
    ...(live.drift ?? []).filter((d) => d.state === 'tripped' || d.state === 'proposed').map((d) => ({
      tone: d.state === 'tripped' ? 'warn' : 'pend',
      text: d.window?.events
        ? `${d.source} drifting from ${d.parser}: ${fmt.n(d.window.misses)} of the last ${fmt.n(d.window.events)} events miss, baseline ${fmt.pct(d.baseline_rate)}`
        : `${d.source} drifting from ${d.parser}: ${fmt.pct(d.window.rate)} of its window missed, baseline ${fmt.pct(d.baseline_rate)}`,
      href: d.pending_id ? `#/review/${encodeURIComponent(d.pending_id)}` : '#/drift',
      link: d.pending_id ? 'review the update' : 'drift',
    })),
    ...(live.pending.count ? [{ tone: 'pend', text: `${live.pending.count} proposal${live.pending.count === 1 ? '' : 's'} waiting for a human`, href: '#/review', link: 'review' }] : []),
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
  // The queue: the depth right now (v4 frame) filled against capacity, with the high-water
  // mark since start as a rule across it. An older frame has no depth and shows the mark alone.
  const qcap = $derived(m?.queue?.capacity ?? e.queue_capacity ?? live.status?.queue_capacity ?? 0)
  const qhw = $derived(e.queue_high_water ?? 0)
  const qnow = $derived(m?.queue?.depth ?? null)
  const pct = (n) => (qcap ? Math.min(100, (100 * n) / qcap) : 0)

  // The two large rates are the windowed ones the server computes over the frames of the last
  // ten seconds, with the run average since start beside them. Without `rate` in the frame
  // (an older server) the run average stands alone, labelled as it was.
  const rate = $derived(m?.rate ?? null)
  const emittedAvg = $derived(e.elapsed_secs ? e.emitted / e.elapsed_secs : null)
  const over = $derived(rate ? `last ${fmt.f(rate.over_secs, 1)} s` : '')

  let sel = $state(-1)
  let filter = $state('')
  let box = $state(null)
  let innerHeight = $state(800)
  let flaggedOnly = $state(false)
  // Space-separated terms, every one a case-insensitive substring of the whole line: the rule
  // docs/api.md gives the export route, so the export of a filtered view is the view.
  const terms = $derived(filter.trim().toLowerCase().split(/\s+/).filter(Boolean))
  const rows = $derived.by(() => {
    let r = live.tail
    if (flaggedOnly) r = r.filter((x) => x.flags.length)
    if (terms.length) r = r.filter((x) => terms.every((t) => x.text.includes(t)))
    return r
  })
  const flagged = $derived(live.tail.reduce((a, r) => a + (r.flags.length ? 1 : 0), 0))
  const countNote = $derived(
    terms.length && flaggedOnly
      ? `${fmt.n(rows.length)} of ${fmt.n(live.tail.length)} flagged and matching`
      : terms.length
        ? `${fmt.n(rows.length)} of ${fmt.n(live.tail.length)}`
        : flaggedOnly
          ? `${fmt.n(rows.length)} flagged of ${fmt.n(live.tail.length)}`
          : `${fmt.n(rows.length)}`,
  )
  $effect(() => { filter; flaggedOnly; sel = -1 })

  // Export: the output file as the sink wrote it, over this view's raw id range or all of it,
  // with the filter's terms so the file is the rows on screen. It writes nothing, so no
  // confirmation; the anchor carries download, the server names the file.
  let exportOpen = $state(false)
  let format = $state('jsonl')
  let whole = $state(false)
  let dl = $state(null)
  const span = $derived.by(() => {
    if (whole || !rows.length) return null
    let from = rows[0].raw_id, to = rows[0].raw_id
    for (const r of rows) { if (r.raw_id < from) from = r.raw_id; if (r.raw_id > to) to = r.raw_id }
    return { from, to }
  })
  const exportUrl = $derived.by(() => {
    const p = new URLSearchParams({ format })
    if (span) { p.set('from', span.from); p.set('to', span.to) }
    if (terms.length) p.set('q', terms.join(' '))
    return `/api/export?${p}`
  })
  const exportNote = $derived(
    [
      span ? `raw ids ${fmt.n(span.from)} to ${fmt.n(span.to)}` : 'every line in the output file',
      terms.length ? `lines carrying ${terms.join(' and ')}` : null,
      flaggedOnly ? 'flagged-only is a filter of this screen; the export route filters on terms, not flags' : null,
    ].filter(Boolean).join(', '),
  )

  $effect(() => keys((ev) => {
    if (ev.key === '/') { box?.focus(); box?.select(); return true }
    if (ev.key === 'f') { flaggedOnly = !flaggedOnly; return true }
    if (ev.key === 'e') { exportOpen = !exportOpen; return true }
    if (exportOpen && ev.key === 'Enter') { dl?.click(); exportOpen = false; return true }
    if (exportOpen && ev.key === 'Escape') { exportOpen = false; return true }
    if (ev.key === ' ') { live.paused ? resume() : (live.paused = true); return true }
    const was = sel
    const hit = nav(ev, rows.length, sel, (n) => (sel = n), (n) => (location.hash = `#/trace/${rows[n].raw_id}`))
    if (hit && sel !== was && !live.paused) live.paused = true // reading a row holds the tail still
    return hit
  }))
  const deny = (a) => a === 'Denied' || a === 'Blocked' || a === 'Dropped'
</script>

<svelte:window bind:innerHeight />

<section class="hero">
  <div class="rates">
    {#if rate}
      <div class="rate"><b class="num">{fmt.f(rate.framed_per_sec, 0)}<i class="avg">{fmt.f(e.events_per_sec, 0)} since start</i></b><span>events framed per second, {over}</span></div>
      <div class="rate"><b class="num">{fmt.f(rate.emitted_per_sec, 0)}<i class="avg">{fmt.f(emittedAvg, 0)} since start</i></b><span>events emitted per second, {over}</span></div>
    {:else}
      <div class="rate"><b class="num">{fmt.f(e.events_per_sec, 0)}</b><span>events per second</span></div>
      <div class="rate"><b class="num">{fmt.f(e.mb_per_sec, 1)}</b><span>MB per second</span></div>
    {/if}
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
  <div class="queue">
    <span class="lab"><span>queue</span><span>{#if qnow != null}{fmt.n(qnow)} / {fmt.n(qcap)} now, high-water {fmt.n(qhw)}{:else}{fmt.n(qhw)} / {fmt.n(qcap)} high-water{/if}</span></span>
    <span class="track">
      <i style="width:{pct(qnow ?? qhw)}%"></i>
      {#if qnow != null}<i class="hw" style="width:{pct(qhw)}%" title="high-water mark since start"></i>{/if}
    </span>
    <span class="n" class:is-warn={e.backpressure_blocks > 0}>{e.backpressure_blocks > 0 ? `producer blocked ${fmt.n(e.backpressure_blocks)} times` : idle ? 'idle, waiting for input' : `${fmt.n(e.batches)} batches, never full`}</span>
  </div>
</section>

{#if alerts.length}
  <section class="alerts">
    {#each alerts as a}
      <div class="alert {a.tone}"><b>{a.text}</b><a href={a.href}>{a.link}</a></div>
    {/each}
  </section>
{/if}

<section>
  <div class="head">
    <h2>Tail</h2>
    <span class="note">newest first, {countNote} rows, click or Enter traces the event</span>
    <span class="push bar">
      <input type="search" bind:value={filter} bind:this={box} onkeydown={(ev) => { if (ev.key === 'Escape') { filter = ''; ev.currentTarget.blur() } }} placeholder="filter every field  /" size="24" aria-label="Filter the tail" />
      <button class="btn" class:on={flaggedOnly} onclick={() => (flaggedOnly = !flaggedOnly)} aria-pressed={flaggedOnly} title="only the events with at least one flag">Flagged<kbd>f</kbd></button>
      <button class="btn" class:on={exportOpen} onclick={() => (exportOpen = !exportOpen)} aria-expanded={exportOpen}>Export<kbd>e</kbd></button>
      {#if live.paused}<span class="tag warn">held, {fmt.n(live.held)} arrived</span>{/if}
      <button class="btn" class:on={live.paused} onclick={() => (live.paused ? resume() : (live.paused = true))}>{live.paused ? 'Release' : 'Hold'}<kbd>space</kbd></button>
    </span>
  </div>
  {#if exportOpen}
    <div class="export">
      <span class="kinds">
        <button class:on={format === 'jsonl'} onclick={() => (format = 'jsonl')} aria-pressed={format === 'jsonl'}>jsonl</button>
        <button class:on={format === 'csv'} onclick={() => (format = 'csv')} aria-pressed={format === 'csv'}>csv</button>
      </span>
      <span class="kinds">
        <button class:on={!whole} onclick={() => (whole = false)} aria-pressed={!whole}>this view</button>
        <button class:on={whole} onclick={() => (whole = true)} aria-pressed={whole}>everything</button>
      </span>
      <span class="muted sm">{exportNote}</span>
      <a class="btn primary push" href={exportUrl} bind:this={dl} download target="_blank" rel="noopener" onclick={() => (exportOpen = false)}>Download<kbd>Enter</kbd></a>
    </div>
  {/if}
  {#if !rows.length}
    <div class="empty">
      <b>{terms.length ? `Nothing in the tail matches ${terms.join(' ')}.` : flaggedOnly ? `Nothing in the tail is flagged: all ${fmt.n(live.tail.length)} events reached every stage.` : 'No events yet.'}</b>
      <span>{terms.length ? 'Esc clears the filter.' : flaggedOnly ? 'f shows every event again.' : 'The tail fills the moment the engine emits: drop a file into a watched directory or send syslog to the listener in the status line.'}</span>
    </div>
  {:else}
    <div class="tail" style="--cols:6em 12em 13em 12em 6em 14em 7em minmax(0,1fr)">
      <VList items={rows} max={Math.max(330, innerHeight - 420)} {sel}>
        {#snippet header()}
          <div class="vh"><span class="num">raw</span><span>time</span><span>parser</span><span>class</span><span>action</span><span>device</span><span title="the stages that did not reach their outcome; hover a mark for the flag">flags</span><span>summary</span></div>
        {/snippet}
        {#snippet row(r, i)}
          <div class="vr" class:sel={i === sel} onclick={() => (location.hash = `#/trace/${r.raw_id}`)} role="button" tabindex="-1">
            <span class="num">{r.raw_id}</span>
            <span class="mono is-dim">{r.time}</span>
            <span class="mono">{#if r.parser}{r.parser}{:else}<span class="is-warn">{r.status}</span>{/if}</span>
            <span>{r.cls}</span>
            <span class:is-warn={deny(r.action)}>{r.action}</span>
            <span class="mono is-dim">{r.device}</span>
            <Flags flags={r.flags} />
            <span class="mono" title={r.sum}>{r.sum}</span>
          </div>
        {/snippet}
      </VList>
    </div>
  {/if}
</section>

<div class="split sources">
  <section>
    <div class="head"><h2>Sources</h2><span class="note">{fmt.n(m?.sources?.length ?? 0)} seen this run</span></div>
    {#if !m?.sources?.length}
      <div class="empty"><b>No sources yet.</b><span>A source appears when its first file or datagram is read.</span></div>
    {:else}
      <div class="wrap scroll">
        <table class="tbl">
          <thead><tr><th>source</th><th>parser</th><th class="num">events</th><th class="num">detected</th><th class="num">no_parser</th><th class="num">buffered</th><th class="num">window</th><th class="num">baseline</th><th>drift</th><th>proposal</th><th class="fill"></th></tr></thead>
          <tbody>
            {#each m.sources as s (s.name)}
              <tr>
                <td class="mono">{s.name}</td>
                <td class="mono">{#if s.parser}{s.parser}{:else}<span class="is-dim">none</span>{/if}</td>
                <td class="num">{fmt.n(s.events)}</td>
                <td class="num">{fmt.n(s.detected)}</td>
                <td class="num" class:is-warn={s.no_parser > 0}>{fmt.n(s.no_parser)}</td>
                <td class="num">{fmt.n(s.buffered)}</td>
                <td class="num">{s.window_rate == null ? '' : fmt.pct(s.window_rate)}</td>
                <td class="num">{s.baseline_rate == null ? '' : fmt.pct(s.baseline_rate)}</td>
                <td>
                  {#if s.drift === 'tripped'}<span class="tag warn">tripped</span>
                  {:else if s.drift === 'proposed'}<span class="tag pend">proposed</span>
                  {:else if s.drift === 'watching'}<span class="tag">watching</span>
                  {:else}<span class="is-dim">–</span>{/if}
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
      <div class="empty"><b>No parsers loaded.</b><span>The registry scans the parsers directory at start and whenever it changes.</span></div>
    {:else}
      <div class="wrap scroll">
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
  <div class="head"><h2>Engine</h2><span class="note">every counter the run block prints, live</span></div>
  <div class="counters">
    <b>input</b>
    <span class="kvs">
      <span class="kv"><span>files</span><span class="num">{fmt.n(e.files)}</span></span>
      <span class="kv" class:bad={e.files_failed > 0}><span>failed</span><span class="num">{fmt.n(e.files_failed)}</span></span>
      <span class="kv"><span>MB in</span><span class="num">{fmt.mb(e.bytes)}</span></span>
      <span class="kv"><span>MB per second</span><span class="num">{fmt.f(e.mb_per_sec, 1)}</span></span>
      <span class="kv"><span>output bytes</span><span class="num">{fmt.n(e.output_bytes)}</span></span>
      <span class="kv"><span>elapsed</span><span class="num">{fmt.f(e.elapsed_secs, 1)}s</span></span>
      <span class="kv"><span>threads</span><span class="num">{fmt.n(e.threads)}</span></span>
      <span class="kv" class:bad={e.parse_failed?.length}><span>parse_failed</span><span class="num">{fmt.pairs(e.parse_failed)}</span></span>
    </span>
    <b>signals</b>
    <span class="kvs">
      {#each ['sub_matched', 'sub_no_match', 'sub_uncovered', 'time_from_receipt', 'class_unknown', 'enum_other', 'unmapped_fields', 'utf8_lossy'] as k}
        <span class="kv" class:on={e[k] > 0 && k !== 'sub_matched'}><span>{k}</span><span class="num">{fmt.n(e[k])}</span></span>
      {/each}
      <span class="kv" class:on={e.time_error?.length}><span>time_error</span><span class="num">{fmt.pairs(e.time_error)}</span></span>
    </span>
    <b>queue</b>
    <span class="kvs">
      <span class="kv"><span>batches</span><span class="num">{fmt.n(e.batches)}</span></span>
      <span class="kv"><span>high-water</span><span class="num">{fmt.n(e.queue_high_water)}/{fmt.n(e.queue_capacity)}</span></span>
      <span class="kv" class:on={e.backpressure_blocks > 0}><span>backpressure blocks</span><span class="num">{fmt.n(e.backpressure_blocks)}</span></span>
    </span>
    <b>inference</b>
    <span class="kvs">
      {#each ['infer_buffered', 'infer_buffer_full', 'infer_runs', 'infer_lines_templated', 'infer_lines_unmatched', 'proposals_written', 'proposals_replaced', 'approved', 'rejected', 'reloads'] as k}
        <span class="kv" class:ok={k === 'approved' && e[k] > 0}><span>{k}</span><span class="num">{fmt.n(e[k])}</span></span>
      {/each}
      <span class="kv"><span>skipped</span><span class="num">{fmt.pairs(e.proposals_skipped)}</span></span>
    </span>
    {#if e.drift_tripped != null}
      <b>drift</b>
      <span class="kvs">
        {#each ['drift_tripped', 'drift_lines_routed', 'drift_proposals', 'drift_cleared'] as k}
          <span class="kv" class:on={k === 'drift_tripped' && e[k] > 0}><span>{k.replace('drift_', '')}</span><span class="num">{fmt.n(e[k])}</span></span>
        {/each}
      </span>
    {/if}
    {#if m?.syslog || e.syslog_udp_datagrams != null}
      <b>syslog</b>
      <span class="kvs">
        <span class="kv"><span>udp datagrams</span><span class="num">{fmt.n(m?.syslog?.udp_datagrams ?? e.syslog_udp_datagrams)}</span></span>
        <span class="kv"><span>udp bytes</span><span class="num">{fmt.n(e.syslog_udp_bytes)}</span></span>
        <span class="kv"><span>tcp connections</span><span class="num">{fmt.n(m?.syslog?.tcp_connections ?? e.syslog_tcp_connections)}</span></span>
        <span class="kv"><span>tcp events</span><span class="num">{fmt.n(m?.syslog?.tcp_events ?? e.syslog_tcp_events)}</span></span>
        <span class="kv" class:on={e.syslog_tcp_partial > 0}><span>tcp partial</span><span class="num">{fmt.n(e.syslog_tcp_partial)}</span></span>
        <span class="kv" class:bad={e.syslog_errors > 0}><span>errors</span><span class="num">{fmt.n(e.syslog_errors)}</span></span>
      </span>
    {/if}
    {#if live.integrity}
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
          <span class="kv"><span>at</span><span class="num">{fmt.stamp(live.integrity.last_verify.at)}</span></span>
        {:else}
          <span class="kv"><span>last verify</span><span class="num is-dim">never</span></span>
        {/if}
      </span>
    {/if}
    {#if m?.pivot}
      <b>pivot</b>
      <span class="kvs">
        <span class="kv"><span>postings</span><span class="num">{fmt.n(m.pivot.postings)}</span></span>
        <span class="kv"><span>batches</span><span class="num">{fmt.n(m.pivot.batches)}</span></span>
        <span class="kv" class:on={m.pivot.blocked > 0}><span>blocked</span><span class="num">{fmt.n(m.pivot.blocked)}</span></span>
        <span class="kv" class:bad={m.pivot.errors > 0}><span>errors</span><span class="num">{fmt.n(m.pivot.errors)}</span></span>
      </span>
    {/if}
    {#if m?.replay}
      <b>replay</b>
      <span class="kvs">
        <span class="kv"><span>latest version</span><span class="num">{m.replay.last_version == null ? 'none yet' : `v${fmt.n(m.replay.last_version)}`}</span></span>
        <span class="kv" class:on={m.replay.running}><span>state</span><span class="num">{m.replay.running ? 'running' : 'idle'}</span></span>
      </span>
    {/if}
  </div>
</section>
