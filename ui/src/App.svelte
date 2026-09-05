<script>
  import Live from './Live.svelte'
  import Review from './Review.svelte'
  import Traceback from './Traceback.svelte'
  import Pivot from './Pivot.svelte'
  import Replay from './Replay.svelte'
  import Drift from './Drift.svelte'
  import Integrity from './Integrity.svelte'
  import { live, loadStatus } from './state.svelte.js'
  import { screenKey, typing } from './keys.js'
  import { fmt } from './api.js'

  const SCREENS = [
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
    const view = SCREENS.some((s) => s.view === parts[0]) ? parts[0] : 'live'
    return { view, a: parts[1] ?? '', b: parts.slice(2).join('/') }
  }
  let route = $state(parse(location.hash))
  let helpOpen = $state(false)
  window.addEventListener('hashchange', () => { route = parse(location.hash); helpOpen = false })
  loadStatus()

  function onKey(e) {
    if (e.metaKey || e.ctrlKey || e.altKey) return
    if (e.key === 'Escape') {
      if (helpOpen) { helpOpen = false; e.preventDefault(); return }
      if (typing(e)) { e.target.blur(); return }
    }
    if (typing(e)) return
    if (e.key === '?' || (e.key === '/' && e.shiftKey)) { helpOpen = !helpOpen; e.preventDefault(); return }
    if (helpOpen) { if (e.key === 'Escape') helpOpen = false; return }
    const s = SCREENS.find((x) => x.key === e.key)
    if (s) { location.hash = `#/${s.view}`; e.preventDefault(); return }
    if (screenKey(e)) e.preventDefault()
  }

  const drifting = $derived((live.drift ?? []).filter((d) => d.state === 'tripped' || d.state === 'proposed').length)
  const st = $derived(live.status)
  const server = $derived(live.metrics?.server)
</script>

<svelte:window onkeydown={onKey} />

<header class="top">
  <span class="brand">ULPF</span>
  <nav>
    {#each SCREENS as s (s.view)}
      <a href="#/{s.view}" class:on={route.view === s.view}>
        <kbd>{s.key}</kbd>{s.label}
        {#if s.view === 'review' && live.pending.count}<span class="badge">{live.pending.count}</span>{/if}
        {#if s.view === 'drift' && drifting}<span class="badge warn">{drifting}</span>{/if}
      </a>
    {/each}
  </nav>
  <span class="spacer"></span>
  <button class="helpkey" onclick={() => (helpOpen = true)}>? keys</button>
</header>

<main>
  {#if route.view === 'live'}
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
</main>

<footer class="foot">
  <span class="st {live.conn}">
    <i class="dot"></i>
    {live.conn === 'live' ? 'stream' : live.conn === 'connecting' ? 'connecting' : `retry ${live.retryIn}s`}
  </span>
  <span>listen <b>{st?.listen ?? '–'}</b></span>
  <span>schema <b>{st?.schema ? `${st.schema.name} ${st.schema.version}` : (st ? 'ocsf' : '–')}</b></span>
  {#if st?.syslog?.udp || st?.syslog?.tcp}
    <span>syslog <b>{[st.syslog.udp && `udp ${st.syslog.udp}`, st.syslog.tcp && `tcp ${st.syslog.tcp}`].filter(Boolean).join('  ')}</b></span>
  {/if}
  <span>up <b>{fmt.ago(server?.uptime_secs)}</b></span>
  <span>clients <b>{fmt.n(server?.sse_clients ?? 0)}</b></span>
  <span class="push" class:is-warn={live.dropped > 0} title="a frame that arrived before the previous one painted replaced it; nothing queues">frames skipped <b>{fmt.n(live.dropped)}</b></span>
  <span class:is-warn={live.skipped > 0} title="events the server's tail ring evicted before this client read them">events skipped <b>{fmt.n(live.skipped)}</b></span>
</footer>

{#if helpOpen}
  <div class="overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) helpOpen = false }}>
    <div class="keymap" role="dialog" aria-modal="true" aria-label="Keyboard map" tabindex="-1">
      <h2>Keys</h2>
      <div class="grp">Anywhere</div>
      <dl>
        <dt>1 … 7</dt><dd>Live, Review, Traceback, Pivot, Replay, Drift, Integrity</dd>
        <dt>?</dt><dd>this map</dd>
        <dt>Esc</dt><dd>close, or leave a detail for its list</dd>
        <dt>/</dt><dd>the search or lookup box on this screen</dd>
      </dl>
      <div class="grp">Any list or table</div>
      <dl>
        <dt>j / k</dt><dd>move down, up (arrows work too)</dd>
        <dt>g / G</dt><dd>first row, last row</dd>
        <dt>Enter</dt><dd>open the selected row</dd>
      </dl>
      <div class="grp">Live</div>
      <dl>
        <dt>space</dt><dd>hold the tail still, and release it</dd>
        <dt>/</dt><dd>filter the tail</dd>
        <dt>Enter</dt><dd>trace the selected event's bytes</dd>
      </dl>
      <div class="grp">Review</div>
      <dl>
        <dt>/</dt><dd>filter the proposal list</dd>
        <dt>s</dt><dd>save the definition</dd>
        <dt>a</dt><dd>approve</dd>
        <dt>x</dt><dd>reject</dd>
        <dt>d</dt><dd>diff against the parser this replaces</dd>
      </dl>
      <div class="grp">Traceback</div>
      <dl>
        <dt>/</dt><dd>the raw id box</dd>
        <dt>j / k</dt><dd>walk the normalized fields, lighting each field's bytes</dd>
        <dt>Enter</dt><dd>keep the selected field lit while you read the other side</dd>
        <dt>h</dt><dd>show the hex dump</dd>
        <dt>Esc</dt><dd>release the lit field</dd>
      </dl>
      <div class="grp">Pivot</div>
      <dl>
        <dt>/</dt><dd>the entity search box</dd>
        <dt>Backspace</dt><dd>back one step along the trail</dd>
        <dt>m</dt><dd>load older events</dd>
      </dl>
      <div class="grp">Replay and Drift</div>
      <dl>
        <dt>j / k</dt><dd>walk the diff entries, or the drift alerts</dd>
        <dt>Enter</dt><dd>trace the entry's bytes, or open the drift proposal</dd>
      </dl>
    </div>
  </div>
{/if}
