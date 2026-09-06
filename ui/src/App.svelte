<script>
  import Live from './Live.svelte'
  import Review from './Review.svelte'
  import Traceback from './Traceback.svelte'
  import Pivot from './Pivot.svelte'
  import Replay from './Replay.svelte'
  import Drift from './Drift.svelte'
  import Integrity from './Integrity.svelte'
  import Flow from './Flow.svelte'
  import { live, loadStatus } from './state.svelte.js'
  import { screenKey, typing, theme } from './keys.js'
  import { fmt } from './api.js'

  // Flow is the front door (#/ and #/flow, key 0, Esc from any top-level screen); the
  // seven windows are one step behind it.
  const SCREENS = [
    { key: '0', view: 'flow', label: 'Flow' },
    { key: '1', view: 'live', label: 'Live' },
    { key: '2', view: 'review', label: 'Review' },
    { key: '3', view: 'trace', label: 'Traceback' },
    { key: '4', view: 'pivot', label: 'Pivot' },
    { key: '5', view: 'replay', label: 'Replay' },
    { key: '6', view: 'drift', label: 'Drift' },
    { key: '7', view: 'integrity', label: 'Integrity' },
  ]

  // #/live · #/review/<id> · #/trace/<raw_id> · #/pivot/<kind>/<value> · #/replay · #/drift · #/integrity
  function parse(h) {
    const parts = h.replace(/^#\/?/, '').split('/').map(decodeURIComponent)
    const view = SCREENS.some((s) => s.view === parts[0]) ? parts[0] : 'flow'
    return { view, a: parts[1] ?? '', b: parts.slice(2).join('/') }
  }
  let route = $state(parse(location.hash))
  let helpOpen = $state(false)
  let mode = $state(theme())
  window.addEventListener('hashchange', () => {
    route = parse(location.hash)
    helpOpen = false
    window.scrollTo(0, 0)
    // The screen transition runs only for a navigation, never for the first paint.
    document.documentElement.dataset.nav = ''
  })
  loadStatus()
  // A count badge pops when it changes, not when it first appears with the hello frame.
  setTimeout(() => (document.documentElement.dataset.live = ''), 1500)

  function onKey(e) {
    if (e.metaKey || e.ctrlKey || e.altKey) return
    if (e.key === 'Escape') {
      if (helpOpen) { helpOpen = false; e.preventDefault(); return }
      if (typing(e)) { e.target.blur(); return }
    }
    if (typing(e)) return
    if (e.key === '?') { helpOpen = !helpOpen; e.preventDefault(); return }
    if (helpOpen) return
    if (e.key === 't') { mode = theme(mode === 'light' ? 'dark' : 'light'); return }
    const s = SCREENS.find((x) => x.key === e.key)
    if (s) { location.hash = `#/${s.view}`; e.preventDefault(); return }
    if (screenKey(e)) { e.preventDefault(); return }
    // An Esc no screen used goes back to the front door.
    if (e.key === 'Escape' && route.view !== 'flow') location.hash = '#/flow'
  }

  const drifting = $derived((live.drift ?? []).filter((d) => d.state === 'tripped' || d.state === 'proposed').length)
  const st = $derived(live.status)
  const server = $derived(live.metrics?.server)
</script>

<svelte:window onkeydown={onKey} />

<header class="top">
  <span class="brand">ULPF</span>
  <nav aria-label="Screens">
    {#each SCREENS as s (s.view)}
      <a href="#/{s.view}" class:on={route.view === s.view} aria-current={route.view === s.view ? 'page' : undefined}>
        <kbd>{s.key}</kbd>{s.label}
        {#if s.view === 'review' && live.pending.count}{#key live.pending.count}<span class="count pop">{live.pending.count}</span>{/key}{/if}
        {#if s.view === 'drift' && drifting}{#key drifting}<span class="count warn pop">{drifting}</span>{/key}{/if}
      </a>
    {/each}
  </nav>
  <span class="right">
    <button onclick={() => (mode = theme(mode === 'light' ? 'dark' : 'light'))} title="t">{mode === 'light' ? 'dark' : 'light'} <kbd class="key">t</kbd></button>
    <button onclick={() => (helpOpen = true)} title="?">keys <kbd class="key">?</kbd></button>
  </span>
</header>

<main>
  {#key route.view}
    <div class="screen">
      {#if route.view === 'flow'}
        <Flow />
      {:else if route.view === 'live'}
        <Live />
      {:else if route.view === 'review'}
        <Review id={route.a} />
      {:else if route.view === 'trace'}
        <Traceback id={route.a} />
      {:else if route.view === 'pivot'}
        <Pivot kind={route.a} value={route.b} />
      {:else if route.view === 'replay'}
        <Replay />
      {:else if route.view === 'drift'}
        <Drift />
      {:else}
        <Integrity />
      {/if}
    </div>
  {/key}
</main>

<footer class="foot">
  <span class={live.conn}><i class="dot"></i>{live.conn === 'live' ? 'stream' : live.conn === 'connecting' ? 'connecting' : `retry ${live.retryIn}s`}</span>
  <span>listen <b>{st?.listen ?? '–'}</b></span>
  <span>schema <b>{st?.schema ? `${st.schema.name} ${st.schema.version}` : (st ? 'ocsf' : '–')}</b></span>
  {#if st?.syslog?.udp || st?.syslog?.tcp}
    <span>syslog <b>{[st.syslog.udp && `udp ${st.syslog.udp}`, st.syslog.tcp && `tcp ${st.syslog.tcp}`].filter(Boolean).join('  ')}</b></span>
  {/if}
  <span>up <b>{fmt.ago(server?.uptime_secs)}</b></span>
  <span>clients <b>{fmt.n(server?.sse_clients ?? 0)}</b></span>
  {#if live.metrics?.queue}<span title="batches in flight between the ingest threads and the output thread, against the queue's capacity">queue <b>{fmt.n(live.metrics.queue.depth)}/{fmt.n(live.metrics.queue.capacity)}</b></span>{/if}
  <span class="push" class:is-warn={live.dropped > 0} title="a frame that arrived before the previous one painted replaced it; nothing queues">frames skipped <b>{fmt.n(live.dropped)}</b></span>
  <span class:is-warn={live.skipped > 0} title="events the server's tail ring evicted before this client read them">events skipped <b>{fmt.n(live.skipped)}</b></span>
</footer>

{#if helpOpen}
  <div class="overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) helpOpen = false }}>
    <div class="keymap" role="dialog" aria-modal="true" aria-label="Keyboard map" tabindex="-1" {@attach (el) => el.focus()}>
      <h2>Keys <span class="note">Esc closes</span></h2>
      <section>
        <h3>Anywhere</h3>
        <dl>
          <dt>0</dt><dd>Flow, the front door: every station of the machine, live</dd>
          <dt>1 … 7</dt><dd>Live, Review, Traceback, Pivot, Replay, Drift, Integrity, one step behind it</dd>
          <dt>?</dt><dd>this map</dd>
          <dt>t</dt><dd>light or dark</dd>
          <dt>/</dt><dd>the search or lookup box on this screen</dd>
          <dt>Esc</dt><dd>close, leave a detail for its list, or go back to Flow from any screen</dd>
        </dl>
        <h3>Flow</h3>
        <dl>
          <dt>i s d p n e</dt><dd>ingest, preserve, detect, parse, normalize, emit: opens the screen behind the station</dd>
          <dt>r</dt><dd>the tray: proposals waiting for review</dd>
          <dt>h / l</dt><dd>move along the line (arrows work too), Enter opens</dd>
        </dl>
        <h3>Any list</h3>
        <dl>
          <dt>j / k</dt><dd>move down, up (arrows work too)</dd>
          <dt>g / G</dt><dd>first row, last row</dd>
          <dt>Enter</dt><dd>open the selected row</dd>
        </dl>
        <h3>Live</h3>
        <dl>
          <dt>space</dt><dd>hold the tail still, and release it</dd>
          <dt>Enter</dt><dd>trace the selected event's bytes</dd>
          <dt>f</dt><dd>only the rows carrying a flag, and every row again</dd>
          <dt>e</dt><dd>the export choice: Enter takes the file, Esc closes it</dd>
        </dl>
        <h3>Traceback</h3>
        <dl>
          <dt>j / k</dt><dd>walk the normalized fields, lighting each field's bytes</dd>
          <dt>Enter</dt><dd>keep the selected field lit while you read the other side</dd>
          <dt>h</dt><dd>hex or text</dd>
          <dt>Esc</dt><dd>release the lit field</dd>
        </dl>
      </section>
      <section>
        <h3>Review</h3>
        <dl>
          <dt>s</dt><dd>save the definition</dd>
          <dt>a</dt><dd>approve: opens the confirmation, Enter confirms, Esc cancels</dd>
          <dt>x</dt><dd>reject: the same confirmation</dd>
          <dt>d</dt><dd>diff against the parser this replaces</dd>
          <dt>m</dt><dd>merge the picked templates</dd>
          <dt>r</dt><dd>regenerate from the kept templates</dd>
        </dl>
        <h3>Pivot</h3>
        <dl>
          <dt>Backspace</dt><dd>back one step along the trail</dd>
          <dt>m</dt><dd>load older events</dd>
          <dt>Enter</dt><dd>trace the selected event</dd>
        </dl>
        <h3>Replay, Drift, Integrity</h3>
        <dl>
          <dt>j / k</dt><dd>walk the diff entries, or the drift alerts</dd>
          <dt>Enter</dt><dd>trace the entry, or open the drift proposal</dd>
          <dt>v</dt><dd>start a verify (Integrity) or a replay (Replay), with confirmation</dd>
          <dt>m</dt><dd>load more diff entries (Replay)</dd>
        </dl>
      </section>
    </div>
  </div>
{/if}
