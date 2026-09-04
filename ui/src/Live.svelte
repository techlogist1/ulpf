<script>
  import { live } from './state.svelte.js'
  import { fmt } from './api.js'

  const m = $derived(live.metrics)
  const e = $derived(live.metrics?.engine ?? {})
  const idle = $derived(!live.metrics || !live.metrics.engine?.framed)

  function open(ev) {
    location.hash = `#/trace/${ev.raw_id}`
  }
  const pick = (o, ...path) => path.reduce((a, k) => (a == null ? a : a[k]), o)
</script>

<section>
  <h2>Engine {#if idle}<span class="muted">idle, waiting for input</span>{/if}</h2>
  <div class="counters">
    <div class="row rate">
      <b>rate</b>
      <span class="kvs">
      <span class="kv"><span>events/s</span><span class="num">{fmt.f(e.events_per_sec, 0)}</span></span>
      <span class="kv"><span>MB/s</span><span class="num">{fmt.f(e.mb_per_sec, 1)}</span></span>
      <span class="kv"><span>files</span><span class="num">{fmt.n(e.files)}</span></span>
      <span class="kv" class:warn={e.files_failed > 0}><span>failed</span><span class="num">{fmt.n(e.files_failed)}</span></span>
      <span class="kv"><span>MB</span><span class="num">{fmt.mb(e.bytes)}</span></span>
      <span class="kv"><span>elapsed s</span><span class="num">{fmt.f(e.elapsed_secs, 1)}</span></span>
      <span class="kv"><span>threads</span><span class="num">{fmt.n(e.threads)}</span></span>
      </span>
    </div>
    <div class="row">
      <b>stages</b>
      <span class="kvs">
      {#each ['framed', 'stored', 'detected', 'no_parser', 'parsed', 'normalized', 'emitted'] as k}
        <span class="kv" class:warn={k === 'no_parser' && e[k] > 0}><span>{k}</span><span class="num">{fmt.n(e[k])}</span></span>
      {/each}
      <span class="kv"><span>output bytes</span><span class="num">{fmt.n(e.output_bytes)}</span></span>
      <span class="kv" class:warn={e.parse_failed?.length}><span>parse_failed</span><span class="num">{fmt.pairs(e.parse_failed)}</span></span>
      </span>
    </div>
    <div class="row">
      <b>signals</b>
      <span class="kvs">
      {#each ['sub_matched', 'sub_no_match', 'sub_uncovered', 'time_from_receipt', 'class_unknown', 'enum_other', 'unmapped_fields', 'utf8_lossy'] as k}
        <span class="kv"><span>{k}</span><span class="num">{fmt.n(e[k])}</span></span>
      {/each}
      <span class="kv" class:warn={e.time_error?.length}><span>time_error</span><span class="num">{fmt.pairs(e.time_error)}</span></span>
      </span>
    </div>
    <div class="row">
      <b>queue</b>
      <span class="kvs">
      <span class="kv"><span>batches</span><span class="num">{fmt.n(e.batches)}</span></span>
      <span class="kv"><span>high-water</span><span class="num">{fmt.n(e.queue_high_water)}/{fmt.n(e.queue_capacity)}</span></span>
      <span class="kv" class:warn={e.backpressure_blocks > 0}><span>backpressure blocks</span><span class="num">{fmt.n(e.backpressure_blocks)}</span></span>
      </span>
    </div>
    <div class="row">
      <b>inference</b>
      <span class="kvs">
      {#each ['infer_buffered', 'infer_buffer_full', 'infer_runs', 'infer_lines_templated', 'infer_lines_unmatched', 'proposals_written', 'proposals_replaced', 'approved', 'rejected', 'reloads'] as k}
        <span class="kv"><span>{k}</span><span class="num">{fmt.n(e[k])}</span></span>
      {/each}
      <span class="kv"><span>proposals_skipped</span><span class="num">{fmt.pairs(e.proposals_skipped)}</span></span>
      </span>
    </div>
  </div>
  {#if m?.server}
    <p class="sm muted">server: {fmt.n(m.server.sse_clients)} stream clients, {fmt.n(m.server.review_errors)} review errors, up {fmt.f(m.server.uptime_secs, 0)} s</p>
  {/if}
</section>

<div class="two">
  <section>
    <h2>Sources</h2>
    {#if !m?.sources?.length}
      <p class="empty">No sources yet. They appear when the first file or watched directory is read.</p>
    {:else}
      <table class="tbl">
        <thead><tr><th>name</th><th class="num">events</th><th class="num">detected</th><th class="num">no_parser</th><th class="num">buffered</th><th>pending</th></tr></thead>
        <tbody>
          {#each m.sources as s (s.name)}
            <tr>
              <td class="mono">{s.name}</td>
              <td class="num">{fmt.n(s.events)}</td>
              <td class="num">{fmt.n(s.detected)}</td>
              <td class="num">{fmt.n(s.no_parser)}</td>
              <td class="num">{fmt.n(s.buffered)}</td>
              <td>{#if s.pending_id}<a href="#/review/{encodeURIComponent(s.pending_id)}" class="mono">{s.pending_id}</a>{:else}<span class="muted">none</span>{/if}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </section>
  <section>
    <h2>Parsers</h2>
    {#if !m?.parsers?.length}
      <p class="empty">No parsers loaded.</p>
    {:else}
      <table class="tbl">
        <thead><tr><th>name</th><th>vendor</th><th>product</th><th class="num">priority</th><th>origin</th><th class="num">detected</th></tr></thead>
        <tbody>
          {#each m.parsers as p (p.name)}
            <tr>
              <td class="mono">{p.name}</td>
              <td>{p.vendor}</td>
              <td>{p.product}</td>
              <td class="num">{fmt.n(p.priority)}</td>
              <td>{p.origin}</td>
              <td class="num">{fmt.n(p.detected)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </section>
</div>

<section>
  <h2>Recent events <span class="muted sm">newest first{live.skipped ? `, ${fmt.n(live.skipped)} skipped by a slow client` : ''}</span></h2>
  {#if !live.tail.length}
    <p class="empty">No events yet. The tail fills as the engine emits.</p>
  {:else}
    <table class="tbl">
      <thead><tr><th class="num">raw_id</th><th>source</th><th>parser</th><th>class</th><th>time</th><th>action</th><th>message</th></tr></thead>
      <tbody>
        {#each live.tail as ev (ev.raw_id)}
          <tr class="click" onclick={() => open(ev)} title="Open in Traceback">
            <td class="num">{ev.raw_id}</td>
            <td class="mono">{pick(ev.line, 'metadata', 'log_name') ?? ''}</td>
            <td class="mono">{pick(ev.line, 'ulpf', 'parser') ?? pick(ev.line, 'ulpf', 'parse_status') ?? ''}</td>
            <td>{ev.line?.class_name ?? ''}</td>
            <td class="mono">{pick(ev.line, 'metadata', 'event_time_rfc3339') ?? fmt.time(ev.line?.time)}</td>
            <td>{ev.line?.action ?? ''}</td>
            <td class="cut mono" title={ev.line?.message}>{fmt.cut(ev.line?.message)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>
