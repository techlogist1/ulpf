<script>
  import Live from './Live.svelte'
  import Review from './Review.svelte'
  import Traceback from './Traceback.svelte'
  import { live } from './state.svelte.js'

  // Routes: #/live, #/review, #/review/<id>, #/trace/<id>
  function parse(h) {
    const [view = 'live', id = ''] = h.replace(/^#\/?/, '').split('/')
    return { view: ['live', 'review', 'trace'].includes(view) ? view : 'live', id: decodeURIComponent(id) }
  }
  let route = $state(parse(location.hash))
  window.addEventListener('hashchange', () => (route = parse(location.hash)))

  const connText = $derived(
    live.conn === 'live' ? 'stream connected' : live.conn === 'connecting' ? 'connecting' : `stream lost, retrying in ${live.retryIn}s`,
  )
</script>

<header class="top">
  <span class="brand">ULPF</span>
  <nav>
    <a href="#/live" class:on={route.view === 'live'}>Live</a>
    <a href="#/review" class:on={route.view === 'review'}>Review{live.pending.count ? ` (${live.pending.count})` : ''}</a>
    <a href="#/trace" class:on={route.view === 'trace'}>Traceback</a>
  </nav>
  <span class="conn {live.conn}"><i></i>{connText}</span>
</header>

<main>
  {#if route.view === 'live'}
    <Live />
  {:else if route.view === 'review'}
    <Review id={route.id} />
  {:else}
    <Traceback id={route.id} />
  {/if}
</main>
