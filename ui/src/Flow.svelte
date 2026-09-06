<script>
  // The front door: six stations on one line, every number a counter the metrics frame
  // carries, every motion a rate the frames give. A pulse is one animated element per link
  // whose speed is the rate law in docs/design.md (Motion); never one element per event.
  import { live } from './state.svelte.js'
  import { fmt } from './api.js'
  import { keys, stations, reduced } from './keys.js'

  const m = $derived(live.metrics)
  const e = $derived(live.metrics?.engine ?? {})
  const failed = $derived((e.parse_failed ?? []).reduce((a, [, n]) => a + n, 0))
  const buffered = $derived((m?.sources ?? []).reduce((a, s) => a + (s.buffered ?? 0), 0))
  const tripped = $derived((live.drift ?? []).filter((d) => d.state === 'tripped' || d.state === 'proposed'))
  const empty = $derived(!!m && !(e.framed > 0))
  const down = $derived(live.conn !== 'live')

  // Rates. The server's own window when the frame carries one (docs/api.md, v4 `rate`),
  // else the difference between the frames of the last two seconds over their interval;
  // `source` says which is on screen. Per station the delta is always computed here: it is
  // what drives that station's pulse.
  const STAGES = ['framed', 'stored', 'detected', 'parsed', 'normalized', 'emitted', 'infer_buffered']
  let hist = [] // plain, not reactive: one entry per metrics frame received
  let lastAt = null
  let rates = $state({ headline: null, over: 0, source: 'waiting for a second frame', at: null, d: {} })
  $effect(() => {
    const eng = live.metrics?.engine
    if (!eng) return
    if (live.conn !== 'live') { hist = []; rates = { headline: null, over: 0, source: 'stream down', at: lastAt, d: {} }; return }
    const now = performance.now()
    lastAt = new Date()
    hist.push(Object.fromEntries([['t', now], ...STAGES.map((k) => [k, eng[k] ?? 0])]))
    if (hist.length > 5) hist.shift()
    const first = hist[0], last = hist[hist.length - 1]
    const over = (last.t - first.t) / 1000
    const d = {}
    for (const k of STAGES) d[k] = over > 0 ? Math.max(0, (last[k] - first[k]) / over) : 0
    const r = live.metrics?.rate
    rates = r?.framed_per_sec != null
      ? { headline: r.framed_per_sec, emitted: r.emitted_per_sec, over: r.over_secs, source: `the server's own window of ${fmt.f(r.over_secs, 1)} s`, at: lastAt, d }
      : { headline: hist.length > 1 ? d.framed : null, emitted: hist.length > 1 ? d.emitted : null, over, source: hist.length > 1 ? `the difference between two frames ${fmt.f(over, 1)} s apart` : 'waiting for a second frame', at: lastAt, d }
  })

  // The queue: instantaneous depth when the frame carries it (v4 `queue`), else only the
  // high-water mark this run reached, and the label says the depth is unknown.
  const qcap = $derived(m?.queue?.capacity ?? e.queue_capacity ?? live.status?.queue_capacity ?? 0)
  const qdepth = $derived(m?.queue?.depth ?? null)
  const qhw = $derived(e.queue_high_water ?? 0)

  // Speed law: px/s = 16 · log10(1 + events/s). One order of magnitude more is one step
  // faster; 1 event/s still crawls, 400,000/s is not a blur. The animation is 32 px per
  // second at playbackRate 1, so playbackRate = speed / 32.
  const speed = (rate) => (rate > 0 ? (16 * Math.log10(1 + rate)) / 32 : 0)
  const anims = {}
  const pulse = (el) => {
    if (reduced()) return
    const v = el.dataset.axis === 'y'
    const a = el.animate([{ transform: 'translate(0,0)' }, { transform: v ? 'translate(0,-32px)' : 'translate(-32px,0)' }], { duration: 1000, iterations: Infinity })
    a.playbackRate = 0
    anims[el.dataset.k] = a
    return () => { a.cancel(); delete anims[el.dataset.k] }
  }
  $effect(() => {
    for (const [k, a] of Object.entries(anims)) a.playbackRate = down ? 0 : speed(rates.d[k] ?? 0)
  })

  // Station to screen: the screen behind each is the one that shows that stage's evidence.
  const latestTrace = $derived(live.latest != null ? `#/trace/${live.latest}` : '#/trace')
  const list = $derived([
    { id: 'ingest', key: 'i', href: '#/live', screen: 'Live' },
    { id: 'preserve', key: 's', href: '#/integrity', screen: 'Integrity' },
    { id: 'detect', key: 'd', href: '#/drift', screen: 'Drift' },
    { id: 'parse', key: 'p', href: latestTrace, screen: 'Traceback' },
    { id: 'normalize', key: 'n', href: '#/pivot', screen: 'Pivot' },
    { id: 'emit', key: 'e', href: '#/replay', screen: 'Replay' },
    { id: 'tray', key: 'r', href: '#/review', screen: 'Review' },
  ])
  let sel = $state(-1)
  const open = (s) => (location.hash = s.href)
  $effect(() => keys((ev) => stations(ev, list, sel, (n) => (sel = n), open)))
  const at = (id) => list.findIndex((s) => s.id === id)
  const isSel = (id) => sel === at(id)

  // The chain strip: one mark per attestation checkpoint (every 4,096 records, the unit
  // `ulpf attest` exports), the newest lit while records are still arriving.
  const every = $derived(live.integrity?.checkpoint_every ?? 4096)
  const marks = $derived(Math.min(24, Math.ceil((live.integrity?.records ?? 0) / every)))
  const grew = $derived(rates.d.stored > 0)
  const out = $derived(String(live.status?.output ?? 'out.jsonl').split('/').pop())
  const seenCount = live.pending.count // the tray badge pops when the count changes, not when the screen opens
</script>

<section class="flow">
  <div class="head">
    <h2>Flow</h2>
    <span class="note">every event through the machine, live; click a station or press its key for the screen behind it, 0 or Esc returns here</span>
  </div>

  {#if down && m}
    <div class="notice warn"><b>The stream dropped{live.retryIn ? `; reconnecting in ${live.retryIn} s` : ', reconnecting'}.</b><span>The numbers are from the last frame{rates.at ? ` at ${rates.at.toTimeString().slice(0, 8)}` : ''}; the pulses stop until frames arrive again.</span></div>
  {/if}

  {#if !m}
    <p class="loading">reading the first metrics frame from the stream</p>
  {:else}
    <div class="lede">
      <b class="num">{rates.headline == null ? '–' : fmt.f(rates.headline, 0)}</b>
      <span>events per second into the machine{rates.emitted != null ? `, ${fmt.f(rates.emitted, 0)} out` : ''}<br /><span class="src">rate from {rates.source}</span></span>
    </div>

    <div class="line">
      <!-- ingest -->
      <a class="station" class:sel={isSel('ingest')} href="#/live" title="Live: the tail, every source and every counter">
        <span class="name">ingest<kbd>i</kbd></span>
        <b class="num">{fmt.n(e.framed)}</b>
        <span class="lab">framed</span>
        <span class="sub" title="sources that produced an event since this run started">{fmt.n(m.sources?.length ?? 0)} source{m.sources?.length === 1 ? '' : 's'} this run</span>
        <span class="sub" title="files found in the watched directories, whether or not this run read from them (plus one per syslog listener)">{fmt.n(e.files)} file{e.files === 1 ? '' : 's'} watched</span>
        {#if m.syslog?.udp_datagrams || m.syslog?.tcp_events}<span class="sub">syslog {fmt.n((m.syslog.udp_datagrams ?? 0) + (m.syslog.tcp_events ?? 0))} received</span>{/if}
      </a>
      <span class="link" class:idle={!(rates.d.stored > 0)}><i class="pulse" data-k="stored" data-axis="x" {@attach pulse}></i></span>
      <!-- preserve -->
      <a class="station" class:sel={isSel('preserve')} href="#/integrity" title="Integrity: the chain, verify, the attestation">
        <span class="name">preserve<kbd>s</kbd></span>
        <b class="num">{fmt.n(e.stored)}</b>
        <span class="lab">stored and chained</span>
      </a>
      <span class="link" class:idle={!(rates.d.detected > 0)}><i class="pulse" data-k="detected" data-axis="x" {@attach pulse}></i></span>
      <!-- detect -->
      <a class="station" class:sel={isSel('detect')} href="#/drift" title="Drift: which parser claims each source, and when it stops">
        <span class="name">detect<kbd>d</kbd></span>
        <b class="num">{fmt.n(e.detected)}</b>
        <span class="lab">a parser claimed</span>
        {#if e.no_parser > 0}<span class="loss">−{fmt.n(e.no_parser)} no parser</span>{/if}
        {#each tripped as d (d.source)}
          <span class="tag warn" title="{d.source} drifting from {d.parser}: window {fmt.pct(d.window?.rate)}, baseline {fmt.pct(d.baseline_rate)}">{d.state}: {d.source}</span>
        {/each}
      </a>
      <span class="link" class:idle={!(rates.d.parsed > 0)}><i class="pulse" data-k="parsed" data-axis="x" {@attach pulse}></i></span>
      <!-- parse -->
      <a class="station" class:sel={isSel('parse')} href={latestTrace} title="Traceback: the newest record's bytes with every parsed field lit">
        <span class="name">parse<kbd>p</kbd></span>
        <b class="num">{fmt.n(e.parsed)}</b>
        <span class="lab">the device's fields</span>
        {#if failed > 0}<span class="loss" title={fmt.pairs(e.parse_failed)}>−{fmt.n(failed)} parse failed</span>{/if}
      </a>
      <span class="link" class:idle={!(rates.d.normalized > 0)}><i class="pulse" data-k="normalized" data-axis="x" {@attach pulse}></i></span>
      <!-- normalize -->
      <a class="station" class:sel={isSel('normalize')} href="#/pivot" title="Pivot: one entity across every device, from the normalized fields">
        <span class="name">normalize<kbd>n</kbd></span>
        <b class="num">{fmt.n(e.normalized)}</b>
        <span class="lab">{live.status?.schema?.name ?? 'schema'} fields</span>
      </a>
      <span class="link" class:idle={!(rates.d.emitted > 0)}><i class="pulse" data-k="emitted" data-axis="x" {@attach pulse}></i></span>
      <!-- emit -->
      <a class="station" class:sel={isSel('emit')} href="#/replay" title="Replay: the output versions and what changed between them">
        <span class="name">emit<kbd>e</kbd></span>
        <b class="num">{fmt.n(e.emitted)}</b>
        <span class="lab">lines in {out}</span>
        <span class="sub">{fmt.mb(e.output_bytes)} MB written</span>
      </a>

      <!-- under the first link: the queue between the ingest thread and the workers -->
      <div class="under queue" style="grid-column: 2">
        <span class="lab"><span>queue</span>{#if qdepth != null}<span>{fmt.n(qdepth)} of {fmt.n(qcap)}</span>{/if}</span>
        <span class="track">
          {#if qdepth != null}<i style="width:{qcap ? Math.min(100, (100 * qdepth) / qcap) : 0}%"></i>{/if}
          <i class="hw" style="width:{qcap ? Math.min(100, (100 * qhw) / qcap) : 0}%"></i>
        </span>
        <span class="n" class:is-warn={e.backpressure_blocks > 0} title="the deepest the queue has been since this run started, against its capacity">high-water {fmt.n(qhw)}</span>
        <span class="n" class:is-warn={e.backpressure_blocks > 0} title="the ingest thread blocks when the queue is full: nothing is ever dropped">{e.backpressure_blocks > 0 ? `blocked ${fmt.n(e.backpressure_blocks)} times` : qdepth == null ? 'depth unreported' : 'never full'}</span>
      </div>

      <!-- under preserve: the chain growing -->
      <a class="under chain" style="grid-column: 3" href="#/integrity">
        <span class="lab"><span>chain</span><span class="mono">{live.integrity?.head ? fmt.hex(live.integrity.head) : 'genesis'}</span></span>
        <span class="marks" class:grew>{#each { length: marks } as _, i}<i class:new={i === marks - 1}></i>{/each}{#if marks === 0}<i class="none"></i>{/if}</span>
        <span class="n">{fmt.n(live.integrity?.records ?? e.stored)} records</span>
        {#if live.integrity?.running || live.integrity?.last_verify}
          <span class="n">{live.integrity.running ? 'verifying' : live.integrity.last_verify.ok ? 'verify clean' : `broken at ${fmt.n(live.integrity.last_verify.first_bad)}`}</span>
        {/if}
      </a>

      <!-- under detect: the inference branch and the tray -->
      <div class="under branch" style="grid-column: 5" class:lit={buffered > 0 || rates.d.infer_buffered > 0 || live.pending.count > 0}>
        <span class="link v" class:idle={!(rates.d.infer_buffered > 0)}><i class="pulse" data-k="infer_buffered" data-axis="y" {@attach pulse}></i></span>
        <span class="node">
          <span class="lab">inference</span>
          <span class="n">{fmt.n(buffered)} buffered now, {fmt.n(e.infer_buffered)} ever, {fmt.n(e.infer_runs)} run{e.infer_runs === 1 ? '' : 's'}</span>
        </span>
        <span class="link v" class:idle={!(live.pending.count > 0)}><i></i></span>
        <a class="node tray" class:sel={isSel('tray')} href="#/review" title="Review: proposals waiting for a human">
          <span class="lab">tray<kbd>r</kbd></span>
          <span class="n">{#key live.pending.count}<span class="count" class:pop={live.pending.count !== seenCount} class:none={!live.pending.count}>{fmt.n(live.pending.count)}</span>{/key} proposal{live.pending.count === 1 ? '' : 's'} waiting{#if e.approved > 0}, {fmt.n(e.approved)} approved{/if}</span>
        </a>
      </div>

      <!-- under parse: the failures by reason -->
      {#if e.parse_failed?.length}
        <div class="under" style="grid-column: 7"><span class="n is-warn">{fmt.pairs(e.parse_failed)}</span></div>
      {/if}
    </div>

    {#if empty}
      <div class="empty">
        <b>Nothing has moved yet.</b>
        <span>Drop a file into {live.status?.watch?.length ? `the watched director${live.status.watch.length === 1 ? 'y' : 'ies'} ${live.status.watch.join(', ')}` : 'a watched directory'}{live.status?.syslog?.udp || live.status?.syslog?.tcp ? `, or send syslog to ${[live.status.syslog.udp && `udp ${live.status.syslog.udp}`, live.status.syslog.tcp && `tcp ${live.status.syslog.tcp}`].filter(Boolean).join(' or ')}` : ''}. The first event lights every station within 500 ms.</span>
      </div>
    {/if}
  {/if}
</section>
