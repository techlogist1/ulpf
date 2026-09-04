<script>
  import { live } from './state.svelte.js'
  import { api, fmt } from './api.js'

  let { id = '' } = $props()

  let list = $state([])
  let listErr = $state(null)
  let detail = $state(null)
  let detailErr = $state(null)
  let definition = $state('')
  let problems = $state([])
  let keep = $state({})
  let busy = $state('')
  let result = $state(null) // { kind: 'ok'|'bad', title, body }

  async function loadList() {
    const r = await api('GET', '/api/pending')
    if (r.ok) { list = r.data; listErr = null } else listErr = r.data
  }
  async function loadDetail(pid) {
    detail = null; detailErr = null; result = null
    const r = await api('GET', `/api/pending/${encodeURIComponent(pid)}`)
    if (!r.ok) { detailErr = r.data; return }
    detail = r.data
    definition = r.data.definition
    problems = r.data.problems ?? []
    keep = Object.fromEntries((r.data.evidence?.templates ?? []).map((t) => [t.id, true]))
  }

  $effect(() => { live.pending.generation; loadList() })
  $effect(() => { if (id) loadDetail(id) })

  const url = (suffix = '') => `/api/pending/${encodeURIComponent(id)}${suffix}`

  function summary(ev) {
    const parts = [`generated ${ev.generated}`, `${fmt.n(ev.lines_used)} of ${fmt.n(ev.lines_seen)} lines used`]
    if (ev.envelope?.syslog) parts.push(`syslog envelope${ev.envelope.example_header ? ` (${ev.envelope.example_header})` : ''}`)
    if (ev.params) parts.push(`similarity ${ev.params.similarity}`, `min support ${ev.params.min_support}`, `constant share ${ev.params.constant_share}`, `enum max ${ev.params.enum_max}`)
    return parts.join(', ')
  }

  async function save() {
    busy = 'save'
    const r = await api('PUT', url(), { definition })
    busy = ''
    if (r.ok) { problems = r.data.problems ?? []; result = { kind: problems.length ? 'bad' : 'ok', title: problems.length ? 'Saved with problems' : 'Saved' } }
    else result = { kind: 'bad', title: 'Save failed', body: `${r.data.error} (${r.data.reason})` }
  }
  async function regenerate() {
    busy = 'regen'
    const ids = Object.entries(keep).filter(([, v]) => v).map(([k]) => Number(k))
    const r = await api('POST', url('/regenerate'), { keep: ids })
    busy = ''
    if (r.ok) { definition = r.data.definition; problems = r.data.problems ?? []; result = { kind: 'ok', title: `Regenerated from ${ids.length} template${ids.length === 1 ? '' : 's'}` } }
    else result = { kind: 'bad', title: 'Regenerate failed', body: `${r.data.error} (${r.data.reason})` }
  }
  async function approve() {
    busy = 'approve'
    const r = await api('POST', url('/approve'))
    busy = ''
    if (r.ok) {
      const d = r.data
      result = { kind: 'ok', title: `Approved as ${d.name}`, body: `written to ${d.path}\nparsers loaded: ${d.parsers_loaded}\nnow detected: ${d.now_detected?.detected} of ${d.now_detected?.tested} buffered lines`, problems: d.problems ?? [] }
      loadList()
    } else result = { kind: 'bad', title: 'Approve refused', body: `${r.data.error} (${r.data.reason})` }
  }
  async function reject() {
    busy = 'reject'
    const r = await api('POST', url('/reject'))
    busy = ''
    if (r.ok) { result = { kind: 'ok', title: `Rejected ${r.data.id}`, body: `moved to ${r.data.moved_to}` }; loadList() }
    else result = { kind: 'bad', title: 'Reject failed', body: `${r.data.error} (${r.data.reason})` }
  }
</script>

{#if !id}
  <section>
    <h2>Pending proposals</h2>
    {#if listErr}
      <p class="notice bad">{listErr.error} ({listErr.reason})</p>
    {:else if !list.length}
      <p class="empty">Nothing to review. Proposals appear here when a source's unknown lines reach the inference threshold.</p>
    {:else}
      <table class="tbl">
        <thead><tr><th>id</th><th>source</th><th>name</th><th>created</th><th class="num">lines</th><th class="num">templates</th><th class="num">unmatched</th><th>edited</th><th class="num">problems</th></tr></thead>
        <tbody>
          {#each list as p (p.id)}
            <tr class="click" onclick={() => (location.hash = `#/review/${encodeURIComponent(p.id)}`)}>
              <td class="mono">{p.id}</td>
              <td class="mono">{p.source}</td>
              <td class="mono">{p.name}</td>
              <td class="mono">{p.created}</td>
              <td class="num">{fmt.n(p.lines)}</td>
              <td class="num">{fmt.n(p.templates)}</td>
              <td class="num">{fmt.n(p.unmatched)}</td>
              <td>{p.edited ? 'yes' : ''}</td>
              <td class="num" class:problems={p.problems > 0}>{fmt.n(p.problems)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </section>
{:else}
  <section class="bar">
    <a href="#/review">All pending</a>
    <span class="muted">/</span>
    <span class="mono">{id}</span>
    {#if detail}<span class="muted">from source</span><span class="mono">{detail.source}</span>{/if}
  </section>

  {#if detailErr}
    <p class="notice bad">{detailErr.error} ({detailErr.reason})</p>
  {:else if !detail}
    <p class="empty">Loading…</p>
  {:else}
    {@const ev = detail.evidence ?? {}}
    <div class="two">
      <section class="stack">
        <h2>Definition</h2>
        <textarea class="editor" bind:value={definition} spellcheck="false"></textarea>
        <div class="bar">
          <button class="btn primary" onclick={save} disabled={busy !== ''}>Save</button>
          <button class="btn" onclick={approve} disabled={busy !== '' || problems.length > 0} title={problems.length ? 'Fix the problems first' : ''}>Approve</button>
          <button class="btn danger" onclick={reject} disabled={busy !== ''}>Reject</button>
          {#if busy}<span class="muted sm">working…</span>{/if}
        </div>
        {#if problems.length}
          <div class="notice bad">
            <b>{problems.length} problem{problems.length === 1 ? '' : 's'}</b>
            <ul class="problems">{#each problems as p}<li>{p}</li>{/each}</ul>
          </div>
        {/if}
        {#if result}
          <div class="notice {result.kind}">
            <b>{result.title}</b>
            {#if result.body}<pre>{result.body}</pre>{/if}
            {#if result.problems?.length}<ul class="problems">{#each result.problems as p}<li>{p}</li>{/each}</ul>{/if}
          </div>
        {/if}
      </section>

      <section class="stack">
        <h2>Evidence</h2>
        <p class="sm muted">{summary(ev)}</p>

        <div class="bar">
          <h3>Templates ({(ev.templates ?? []).length})</h3>
          <button class="btn" onclick={regenerate} disabled={busy !== ''} style="margin-left:auto">Regenerate from kept</button>
        </div>
        {#each ev.templates ?? [] as t (t.id)}
          <article class="tpl">
            <header>
              <span class="mono">#{t.id}</span>
              <span><span class="muted">support</span> <span class="num">{fmt.n(t.support)}</span></span>
              <span class:problems={t.verified !== t.support}><span class="muted">verified</span> <span class="num">{fmt.n(t.verified)}</span></span>
              <label><input type="checkbox" bind:checked={keep[t.id]} /> Keep</label>
            </header>
            <pre class="pattern">{t.pattern}</pre>
            {#if t.slots?.length}
              <table class="tbl">
                <thead><tr><th>slot</th><th>kind</th><th>preceded by</th><th class="num">distinct</th><th>examples</th></tr></thead>
                <tbody>
                  {#each t.slots as s}
                    <tr>
                      <td class="mono">{#if s.suggested}<span class="suggested" title="Name suggested by the engine, not from the log">{s.name}</span>{:else}{s.name}{/if}</td>
                      <td class="mono">{s.kind}</td>
                      <td class="mono">{s.preceded_by}</td>
                      <td class="num">{fmt.n(s.distinct)}</td>
                      <td class="mono cut" title={(s.examples ?? []).join(' | ')}>{(s.examples ?? []).join(' | ')}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}
            {#if t.examples?.length}
              <div class="ex">{#each t.examples as x}<pre>{x}</pre>{/each}</div>
            {/if}
            {#if t.history?.length}
              <div class="sm muted">{t.history.join('; ')}</div>
            {/if}
          </article>
        {:else}
          <p class="empty">No templates in this proposal.</p>
        {/each}

        <h3>Unmatched ({fmt.n(ev.unmatched?.count)})</h3>
        {#if ev.unmatched}
          <p class="sm muted">{Object.entries(ev.unmatched.by_reason ?? {}).map(([k, v]) => `${k} ${fmt.n(v)}`).join(', ') || 'no reasons recorded'}</p>
          {#if ev.unmatched.examples?.length}<div class="box">{#each ev.unmatched.examples as x}<pre>{x}</pre>{/each}</div>{/if}
        {/if}

        <h3>Decisions</h3>
        {#if ev.decisions?.length}
          <ol class="ol">{#each ev.decisions as d}<li>{d}</li>{/each}</ol>
        {:else}
          <p class="empty">No decisions recorded.</p>
        {/if}
        <p class="sm muted">fingerprint <code>{ev.fingerprint}</code></p>
      </section>
    </div>
  {/if}
{/if}
