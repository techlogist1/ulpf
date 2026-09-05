<script>
  import { live } from './state.svelte.js'
  import { api, fmt } from './api.js'
  import { keys, nav } from './keys.js'

  let { id = '' } = $props()

  let list = $state([])
  let listErr = $state(null)
  let detail = $state(null)
  let detailErr = $state(null)
  let definition = $state('')
  let problems = $state([])
  let keep = $state({})
  let picked = $state({}) // templates selected for one merge group
  let busy = $state('')
  let result = $state(null)
  let showDiff = $state(true)
  let sel = $state(-1)
  let filter = $state('')
  let box = $state(null)

  async function loadList() {
    const r = await api('GET', '/api/pending')
    if (r.ok) { list = r.data; listErr = null } else listErr = r.data
  }
  async function loadDetail(pid) {
    detail = null; detailErr = null; result = null; picked = {}
    const r = await api('GET', `/api/pending/${encodeURIComponent(pid)}`)
    if (!r.ok) { detailErr = r.data; return }
    detail = r.data
    definition = r.data.definition
    problems = r.data.problems ?? []
    keep = Object.fromEntries((r.data.evidence?.templates ?? []).map((t) => [t.id, true]))
  }

  $effect(() => { live.pending.generation; loadList() })
  $effect(() => { if (id) loadDetail(id) })
  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase()
    return q ? list.filter((p) => `${p.id} ${p.source} ${p.name} ${p.updates ?? ''}`.toLowerCase().includes(q)) : list
  })
  $effect(() => { filter; sel = -1 })
  $effect(() => keys((e) => {
    if (!id) {
      if (e.key === '/') { box?.focus(); box?.select(); return true }
      return nav(e, shown.length, sel, (n) => (sel = n), (n) => (location.hash = `#/review/${encodeURIComponent(shown[n].id)}`))
    }
    if (e.key === 'Escape') { location.hash = '#/review'; return true }
    if (e.key === 's') { save(); return true }
    if (e.key === 'a') { approve(); return true }
    if (e.key === 'x') { reject(); return true }
    if (e.key === 'd' && detail?.diff) { showDiff = !showDiff; return true }
    return false
  }))

  const url = (suffix = '') => `/api/pending/${encodeURIComponent(id)}${suffix}`
  const templates = $derived(detail?.evidence?.templates ?? [])
  const pickedIds = $derived(Object.entries(picked).filter(([, v]) => v).map(([k]) => Number(k)))
  const keptIds = $derived(Object.entries(keep).filter(([, v]) => v).map(([k]) => Number(k)))

  async function save() {
    busy = 'save'
    const r = await api('PUT', url(), { definition })
    busy = ''
    if (r.ok) { problems = r.data.problems ?? []; result = { kind: problems.length ? 'bad' : 'ok', title: problems.length ? `Saved, ${problems.length} problem${problems.length === 1 ? '' : 's'} remain` : 'Saved' } }
    else result = { kind: 'bad', title: 'Save failed', body: `${r.data.error} (${r.data.reason})` }
  }
  async function regenerate(merge = []) {
    busy = 'regen'
    const r = await api('POST', url('/regenerate'), { keep: keptIds, merge })
    busy = ''
    if (r.ok) {
      definition = r.data.definition
      problems = r.data.problems ?? []
      result = { kind: 'ok', title: merge.length ? `Merged ${merge[0].length} templates and re-emitted the definition` : `Re-emitted from ${keptIds.length} template${keptIds.length === 1 ? '' : 's'}` }
      picked = {}
      loadDetail(id)
    } else result = { kind: 'bad', title: 'Regenerate failed', body: `${r.data.error} (${r.data.reason})` }
  }
  async function approve() {
    busy = 'approve'
    const r = await api('POST', url('/approve'))
    busy = ''
    if (r.ok) {
      const d = r.data
      result = {
        kind: 'ok', title: `Approved as ${d.name}`,
        body: `written to ${d.path}\n${d.parsers_loaded} parsers loaded${d.replaced_version != null ? `, replaced version ${d.replaced_version}` : ''}\nre-detected ${d.now_detected?.detected} of ${d.now_detected?.tested} buffered lines with the new registry`,
        problems: d.problems ?? [],
      }
      loadList()
    } else result = { kind: 'bad', title: 'Approve refused', body: `${r.data.error} (${r.data.reason})`, problems: r.data.problems ?? [] }
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
    <div class="head">
      <h2>Pending proposals</h2>
      <span class="note">nothing here is parsed until a human approves it</span>
      <span class="push bar">
        <input type="search" bind:value={filter} bind:this={box} onkeydown={(ev) => { if (ev.key === 'Escape') { filter = ''; ev.currentTarget.blur() } }} placeholder="filter by id, source or name  /" size="28" aria-label="Filter proposals" />
      </span>
    </div>
    {#if listErr}
      <p class="notice bad">{listErr.error} ({listErr.reason})</p>
    {:else if !shown.length}
      <p class="empty">{filter.trim() ? `No proposal matches ${filter.trim()}.` : 'Nothing to review. A proposal appears when a source\u2019s unknown lines reach the inference threshold, or when an established source drifts.'}</p>
    {:else}
      <div class="wrap"><table class="tbl">
        <thead><tr><th>id</th><th>source</th><th>proposed name</th><th>kind</th><th>created</th><th class="num">lines</th><th class="num">templates</th><th class="num">unmatched</th><th>edited</th><th class="num">problems</th><th class="fill"></th></tr></thead>
        <tbody>
          {#each shown as p, i (p.id)}
            <tr class="click" class:sel={i === sel} onclick={() => (location.hash = `#/review/${encodeURIComponent(p.id)}`)}>
              <td class="mono">{p.id}</td>
              <td class="mono">{p.source}</td>
              <td class="mono">{p.name}</td>
              <td>{#if p.updates}<span class="tag warn">update v{p.current_version ?? 1} to v{p.version ?? 2}</span>{:else}<span class="tag accent">new parser</span>{/if}</td>
              <td class="mono is-dim">{p.created}</td>
              <td class="num">{fmt.n(p.lines)}</td>
              <td class="num">{fmt.n(p.templates)}</td>
              <td class="num" class:is-warn={p.unmatched > 0}>{fmt.n(p.unmatched)}</td>
              <td>{#if p.edited}<span class="tag">edited</span>{/if}</td>
              <td class="num" class:is-bad={p.problems > 0}>{fmt.n(p.problems)}</td>
              <td class="fill"></td>
            </tr>
          {/each}
        </tbody>
      </table></div>
    {/if}
  </section>
{:else}
  <section class="bar">
    <a href="#/review">All pending</a>
    <span class="muted">/</span>
    <span class="mono">{id}</span>
    {#if detail}
      <span class="muted sm">source</span><span class="mono">{detail.source}</span>
      {#if detail.updates}<span class="tag warn">updates {detail.updates}: v{detail.current_version} to v{detail.version}</span>{/if}
      {#if detail.edited}<span class="tag">edited by hand</span>{/if}
    {/if}
  </section>

  {#if detailErr}
    <p class="notice bad">{detailErr.error} ({detailErr.reason})</p>
  {:else if !detail}
    <p class="empty">Loading.</p>
  {:else}
    {@const ev = detail.evidence ?? {}}
    {#if detail.diff && showDiff}
      <section>
        <div class="head">
          <h2>What changes in {detail.updates}</h2>
          <span class="note">version {detail.current_version} on disk, version {detail.version} proposed</span>
          <span class="push"><button class="btn" onclick={() => (showDiff = false)}>Hide diff</button></span>
        </div>
        <div class="diff">
          {#each detail.diff.split('\n') as l}
            <div class={l.startsWith('+') && !l.startsWith('+++') ? 'add' : l.startsWith('-') && !l.startsWith('---') ? 'del' : l.startsWith('@@') ? 'hunk' : ''}>{l || ' '}</div>
          {/each}
        </div>
      </section>
    {:else if detail.diff}
      <section class="bar"><button class="btn" onclick={() => (showDiff = true)}>Show the diff against {detail.updates} v{detail.current_version}</button></section>
    {/if}

    <div class="split review">
      <section class="stack">
        <div class="head">
          <h2>Definition</h2>
          <span class="note">{detail.updates ? `overwrites parsers/${detail.updates}.toml on approval` : 'written to parsers/ on approval'}</span>
        </div>
        <textarea class="editor" bind:value={definition} spellcheck="false"></textarea>
        <div class="bar">
          <button class="btn primary" onclick={save} disabled={busy !== ''}>Save</button>
          <button class="btn" onclick={approve} disabled={busy !== '' || problems.length > 0} title={problems.length ? 'Fix the problems first' : 'Write it to parsers/ and reload'}>Approve</button>
          <button class="btn danger" onclick={reject} disabled={busy !== ''}>Reject</button>
          {#if busy}<span class="muted sm">{busy}…</span>{/if}
          <span class="muted sm push">keys: s save, a approve, x reject</span>
        </div>
        {#if problems.length}
          <div class="notice bad">
            <b>{problems.length} problem{problems.length === 1 ? '' : 's'} in the text as it stands</b>
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
        {#if detail.current_definition}
          <details>
            <summary class="sm muted">The parser this replaces, as it is on disk</summary>
            <pre class="panel pad" style="max-height:40vh;overflow:auto">{detail.current_definition}</pre>
          </details>
        {/if}
      </section>

      <section class="stack">
        <div class="head">
          <h2>Evidence</h2>
          <span class="note">{fmt.n(ev.lines_used)} of {fmt.n(ev.lines_seen)} lines used, generated {ev.generated}</span>
        </div>
        <div class="bar sm muted">
          {#if ev.envelope?.syslog}<span class="tag">syslog envelope</span>{/if}
          {#if ev.envelope?.example_header}<span class="mono">{ev.envelope.example_header}</span>{/if}
          {#if ev.params}
            <span>similarity {ev.params.similarity}</span><span>min support {ev.params.min_support}</span>
            <span>rare share {ev.params.rare_share}</span><span>enum max {ev.params.enum_max}</span><span>max templates {ev.params.max_templates}</span>
          {/if}
        </div>

        <div class="bar">
          <h3>Templates ({templates.length})</h3>
          <span class="push bar">
            {#if pickedIds.length > 1}
              <button class="btn primary" onclick={() => regenerate([pickedIds])} disabled={busy !== ''}>Merge {pickedIds.length} into one</button>
            {:else if pickedIds.length === 1}
              <span class="muted sm">pick a second template to merge</span>
            {/if}
            <button class="btn" onclick={() => regenerate([])} disabled={busy !== ''}>Regenerate from {keptIds.length} kept</button>
          </span>
        </div>

        {#each templates as t (t.id)}
          <article class="tpl" class:kept={keep[t.id]} class:picked={picked[t.id]}>
            <header>
              <span class="mono">#{t.id}</span>
              <span class="sm"><span class="muted">support</span> <span class="num">{fmt.n(t.support)}</span></span>
              <span class="sm" class:is-warn={t.verified !== t.support} class:is-bad={t.verified === 0}><span class="muted">verified</span> <span class="num">{fmt.n(t.verified)}</span></span>
              {#if t.verified === 0}<span class="tag bad">left out of the definition</span>{/if}
              <span class="sm muted">{fmt.n(t.members?.length)} members</span>
              <span class="push bar">
                <label class="chk"><input type="checkbox" bind:checked={picked[t.id]} /> merge</label>
                <label class="chk"><input type="checkbox" bind:checked={keep[t.id]} /> keep</label>
              </span>
            </header>
            <pre class="pattern">{t.pattern}</pre>
            {#if t.slots?.length}
              <div class="wrap"><table class="tbl">
                <thead><tr><th>slot</th><th>kind</th><th>why this name</th><th>preceded by</th><th class="num">distinct</th><th>examples</th></tr></thead>
                <tbody>
                  {#each t.slots as s}
                    <tr>
                      <td class="mono">{s.name}{#if s.suggested}<span class="suggested" title="a rule produced this name"> ✓</span>{/if}</td>
                      <td class="mono is-dim">{s.kind}</td>
                      <td class="reason">{s.reason ?? (s.suggested ? 'suggested by a rule' : 'generic name: no rule fired')}</td>
                      <td class="mono is-dim">{s.preceded_by}</td>
                      <td class="num">{fmt.n(s.distinct)}</td>
                      <td class="mono is-dim" title={(s.examples ?? []).join(' | ')}>{fmt.cut((s.examples ?? []).join(' | '), 48)}</td>
                    </tr>
                  {/each}
                </tbody>
              </table></div>
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
          <div class="bar sm">
            {#each Object.entries(ev.unmatched.by_reason ?? {}) as [k, v]}
              <span class="tag" class:warn={v > 0}>{k} {fmt.n(v)}</span>
            {/each}
          </div>
          {#if ev.unmatched.examples?.length}<div class="panel pad">{#each ev.unmatched.examples as x}<pre>{x}</pre>{/each}</div>{/if}
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
