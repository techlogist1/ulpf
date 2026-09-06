<script>
  import { live } from './state.svelte.js'
  import { api, fmt } from './api.js'
  import { keys, nav } from './keys.js'
  import Confirm from './Confirm.svelte'

  let { id = '' } = $props()

  let list = $state(null)
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
  let asking = $state(null) // 'approve' | 'reject' | null
  let sel = $state(-1)
  let filter = $state('')
  let box = $state(null)

  async function loadList() {
    const r = await api('GET', '/api/pending')
    if (r.ok) { list = r.data; listErr = null } else listErr = r.data
  }
  async function loadDetail(pid) {
    detail = null; detailErr = null; result = null; picked = {}; asking = null
    const r = await api('GET', `/api/pending/${encodeURIComponent(pid)}`)
    if (!r.ok) { detailErr = r.data; return }
    detail = r.data
    definition = r.data.definition
    problems = r.data.problems ?? []
    keep = Object.fromEntries((r.data.evidence?.templates ?? []).map((t) => [t.id, t.verified > 0]))
  }

  $effect(() => { live.pending.generation; loadList() })
  $effect(() => { if (id) loadDetail(id) })
  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase()
    return q ? (list ?? []).filter((p) => `${p.id} ${p.source} ${p.name} ${p.updates ?? ''}`.toLowerCase().includes(q)) : (list ?? [])
  })
  $effect(() => { filter; sel = -1 })
  $effect(() => keys((e) => {
    if (!id) {
      if (e.key === '/') { box?.focus(); box?.select(); return true }
      return nav(e, shown.length, sel, (n) => (sel = n), (n) => (location.hash = `#/review/${encodeURIComponent(shown[n].id)}`))
    }
    if (asking) return false // the confirmation owns Enter and Esc; nothing else reacts
    if (e.key === 'Escape') { location.hash = '#/review'; return true }
    if (!detail || result?.done) return false
    if (e.key === 's') { save(); return true }
    if (e.key === 'a') { if (canApprove) asking = 'approve'; return true }
    if (e.key === 'x') { asking = 'reject'; return true }
    if (e.key === 'd' && detail?.diff) { showDiff = !showDiff; return true }
    if (e.key === 'm' && pickedIds.length > 1) { regenerate([pickedIds]); return true }
    if (e.key === 'r') { regenerate([]); return true }
    return false
  }))

  const url = (suffix = '') => `/api/pending/${encodeURIComponent(id)}${suffix}`
  const templates = $derived(detail?.evidence?.templates ?? [])
  const pickedIds = $derived(Object.entries(picked).filter(([, v]) => v).map(([k]) => Number(k)))
  const keptIds = $derived(Object.entries(keep).filter(([, v]) => v).map(([k]) => Number(k)))
  const canApprove = $derived(busy === '' && problems.length === 0 && !!detail)
  // Directories named in confirmation strings come from the running server, not a literal:
  // the demo uses demo/parsers and demo/pending, the desktop app its data directory's.
  const pdir = $derived(live.status?.parsers_dir ?? 'parsers')
  const pendir = $derived(live.status?.pending_dir ?? 'pending')
  // The name the definition will activate under, read from the text being edited.
  const parserName = $derived((definition.match(/^\s*name\s*=\s*"([^"]+)"/m) ?? [])[1] ?? detail?.name ?? id)

  async function save() {
    busy = 'save'
    const r = await api('PUT', url(), { definition })
    busy = ''
    if (r.ok) { problems = r.data.problems ?? []; result = { kind: problems.length ? 'bad' : 'ok', title: problems.length ? `Saved, ${problems.length} problem${problems.length === 1 ? '' : 's'} remain` : 'Saved' } }
    else result = { kind: 'bad', title: r.data.path ? `Save failed: ${r.data.path}` : 'Save failed', body: `${r.data.error} (${r.data.reason})` }
  }
  async function regenerate(merge = []) {
    busy = 'regen'
    const r = await api('POST', url('/regenerate'), { keep: keptIds, merge })
    busy = ''
    if (r.ok) {
      definition = r.data.definition
      problems = r.data.problems ?? []
      result = { kind: 'ok', title: merge.length ? `Merged ${merge[0].length} templates into one and re-emitted the definition` : `Re-emitted from ${keptIds.length} kept template${keptIds.length === 1 ? '' : 's'}` }
      picked = {}
      loadDetail(id)
    } else result = { kind: 'bad', title: 'Regenerate failed', body: `${r.data.error} (${r.data.reason})` }
  }
  async function approve() {
    asking = null
    busy = 'approve'
    const r = await api('POST', url('/approve'))
    busy = ''
    if (r.ok) {
      const d = r.data
      result = {
        kind: 'ok', done: true, title: `Approved: ${d.name} is active`,
        proof: [
          ['written to', d.path],
          ['parsers loaded', fmt.n(d.parsers_loaded)],
          ...(d.replaced_version != null ? [['replaced version', `v${d.replaced_version}, kept in ${pendir}/approved/`]] : []),
          ['re-detected now', `${fmt.n(d.now_detected?.detected)} of ${fmt.n(d.now_detected?.tested)} buffered lines take the fast path with the new registry`],
        ],
        problems: d.problems ?? [],
      }
      loadList()
    } else result = { kind: 'bad', title: r.status === 409 ? 'Approve refused: an active parser already has this name' : 'Approve refused', body: `${r.data.error} (${r.data.reason})`, problems: r.data.problems ?? [] }
  }
  async function reject() {
    asking = null
    busy = 'reject'
    const r = await api('POST', url('/reject'))
    busy = ''
    if (r.ok) { result = { kind: 'ok', done: true, title: `Rejected ${r.data.id}`, proof: [['moved to', r.data.moved_to], ['remembered', 'an identical later proposal for this source is skipped']] }; loadList() }
    else result = { kind: 'bad', title: 'Reject failed', body: `${r.data.error} (${r.data.reason})` }
  }
  // The confirmation sat where the reader was looking; the result replaces it from above.
  const reveal = (el) => { el.scrollIntoView({ block: 'start' }); el.focus({ preventScroll: true }) }
  const diffClass = (l) => (l.startsWith('+') && !l.startsWith('+++') ? 'add' : l.startsWith('-') && !l.startsWith('---') ? 'del' : l.startsWith('@@') ? 'hunk' : '')
</script>

{#if !id}
  <section>
    <div class="head">
      <h2>Review</h2>
      <span class="note">proposals the engine wrote from unknown lines; nothing here is parsed until a human approves it</span>
      <span class="push bar">
        <input type="search" bind:value={filter} bind:this={box} onkeydown={(ev) => { if (ev.key === 'Escape') { filter = ''; ev.currentTarget.blur() } }} placeholder="filter by id, source or name  /" size="28" aria-label="Filter proposals" />
      </span>
    </div>
    {#if listErr}
      <div class="notice bad"><b>{listErr.error}</b><span class="muted">{listErr.reason}</span></div>
    {:else if !list}
      <p class="loading">reading the pending directory</p>
    {:else if !shown.length}
      <div class="empty">
        <b>{filter.trim() ? `No proposal matches ${filter.trim()}.` : 'Nothing to review.'}</b>
        <span>{filter.trim() ? 'Esc clears the filter.' : 'A proposal appears when a source’s unknown lines reach the inference threshold, and when an established source drifts. Every proposal is three files in the pending directory; approving is the only way one becomes a parser.'}</span>
      </div>
    {:else}
      <div class="wrap"><table class="tbl">
        <thead><tr><th>id</th><th>source</th><th>proposed name</th><th>kind</th><th>created</th><th class="num">lines</th><th class="num">templates</th><th class="num">unmatched</th><th>edited</th><th class="num">problems</th><th class="fill"></th></tr></thead>
        <tbody>
          {#each shown as p, i (p.id)}
            <tr class="click" class:sel={i === sel} onclick={() => (location.hash = `#/review/${encodeURIComponent(p.id)}`)}>
              <td class="mono">{p.id}</td>
              <td class="mono">{p.source}</td>
              <td class="mono">{p.name}</td>
              <td>{#if p.updates}<span class="tag warn">update v{p.current_version ?? 1} to v{p.version ?? 2}</span>{:else}<span class="tag pend">new parser</span>{/if}</td>
              <td class="mono is-dim">{fmt.stamp(p.created)}</td>
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
      <p class="sm muted" style="margin-top:var(--s4)">j and k move, Enter opens. Inside a proposal: s saves the text, a approves and x rejects, both through a confirmation.</p>
    {/if}
  </section>
{:else}
  <section class="trail">
    <a href="#/review">Review</a>
    <span class="sep">/</span>
    <span class="cur">{id}</span>
    {#if detail}
      <span class="sep">from</span><span class="mono">{detail.source}</span>
      {#if detail.updates}<span class="tag warn">updates {detail.updates}: v{detail.current_version} to v{detail.version}</span>{/if}
      {#if detail.update_kind}<span class="tag">{detail.update_kind.replace('_', ' ')}</span>{/if}
      {#if detail.edited}<span class="tag">edited by hand</span>{/if}
    {/if}
  </section>

  {#if detailErr}
    <div class="notice bad">
      <b>{detailErr.error}</b>
      <span class="muted">{detailErr.reason}{#if detailErr.status === 404}: it was approved or rejected already, or the id is not a pending proposal{/if}</span>
      <span><a href="#/review">Back to the pending list</a></span>
    </div>
  {:else if !detail}
    <p class="loading">reading proposal {id}</p>
  {:else}
    {@const ev = detail.evidence ?? {}}
    {#if detail.diff && showDiff}
      <section>
        <div class="head">
          <h2>What changes in {detail.updates}</h2>
          <span class="note">version {detail.current_version} on disk, version {detail.version} proposed</span>
          <span class="push"><button class="btn" onclick={() => (showDiff = false)}>Hide diff<kbd>d</kbd></button></span>
        </div>
        <div class="diff">
          {#each detail.diff.split('\n') as l}<div class={diffClass(l)}>{l || ' '}</div>{/each}
        </div>
      </section>
    {:else if detail.diff}
      <section class="bar"><button class="btn" onclick={() => (showDiff = true)}>Show the diff against {detail.updates} v{detail.current_version}<kbd>d</kbd></button></section>
    {/if}

    <div class="split review">
      <section class="stack">
        <div class="head">
          <h2>Definition</h2>
          <span class="note">{detail.updates ? `overwrites ${pdir}/${detail.updates}.toml on approval` : 'written to the parsers directory on approval'}</span>
        </div>
        {#if result?.done}
          <div class="notice {result.kind} arrive" tabindex="-1" {@attach reveal}>
            <b>{result.title}</b>
            {#if result.proof}<div class="proof">{#each result.proof as [k, v]}<span>{k}</span><b>{v}</b>{/each}</div>{/if}
            {#if result.problems?.length}<ul class="problems">{#each result.problems as p}<li>{p}</li>{/each}</ul>{/if}
            <span><a href="#/review">Back to the pending list</a> <span class="muted">or</span> <a href="#/live">watch the parsers table in Live</a></span>
          </div>
          <pre class="panel pad json">{definition}</pre>
        {:else}
          <textarea class="editor" bind:value={definition} spellcheck="false" aria-label="Parser definition"></textarea>
          {#if asking === 'approve'}
            <Confirm title="Approve {parserName} as an active parser?" verb="Approve" onconfirm={approve} oncancel={() => (asking = null)}
                     hint={detail.updates ? `The replaced file is kept in ${pendir}/approved/ and the registry reloads in place. Every event from now on is parsed by the new version.` : 'Generated parsers carry priority -1, so a hand-written parser for the same format still wins. The registry reloads in place; nothing already emitted changes.'}>
              {#if detail.updates}
                <span>overwrites <code>{pdir}/{detail.updates}.toml</code>, version {detail.current_version} to {detail.version}</span>
              {:else}
                <span>writes <code>{pdir}/{parserName}.toml</code> and reloads the registry</span>
              {/if}
              <span>re-detects the {fmt.n(ev.lines_seen)} buffered lines from <code>{detail.source}</code> with the new registry and reports how many now take the fast path</span>
            </Confirm>
          {:else if asking === 'reject'}
            <Confirm title="Reject this proposal?" verb="Reject" danger onconfirm={reject} oncancel={() => (asking = null)}
                     hint="Nothing is parsed differently. The engine remembers the template fingerprint and skips an identical later proposal for this source.">
              <span>moves the three files to <code>{pendir}/rejected/</code></span>
            </Confirm>
          {:else}
            <div class="actions">
              <button class="btn" onclick={save} disabled={busy !== ''}>Save<kbd>s</kbd></button>
              <button class="btn primary" onclick={() => (asking = 'approve')} disabled={!canApprove} title={problems.length ? 'Fix the problems first' : 'Opens a confirmation'}>Approve<kbd>a</kbd></button>
              <button class="btn danger" onclick={() => (asking = 'reject')} disabled={busy !== ''}>Reject<kbd>x</kbd></button>
              {#if busy}<span class="loading">{busy}</span>{/if}
              <span class="hint">approve and reject ask once more before writing</span>
            </div>
          {/if}
          {#if problems.length}
            <div class="notice bad">
              <b>{problems.length} problem{problems.length === 1 ? '' : 's'} in the text as it stands: approval is refused until they are fixed</b>
              <ul class="problems">{#each problems as p}<li>{p}</li>{/each}</ul>
            </div>
          {/if}
          {#if result}
            <div class="notice {result.kind}" tabindex="-1" {@attach reveal}>
              <b>{result.title}</b>
              {#if result.body}<pre>{result.body}</pre>{/if}
              {#if result.problems?.length}<ul class="problems">{#each result.problems as p}<li>{p}</li>{/each}</ul>{/if}
            </div>
          {/if}
        {/if}
        {#if detail.current_definition}
          <details>
            <summary>The parser this replaces, as it is on disk</summary>
            <pre class="panel pad json">{detail.current_definition}</pre>
          </details>
        {/if}
      </section>

      <section class="stack">
        <div class="head">
          <h2>Evidence</h2>
          <span class="note">{fmt.n(ev.lines_used)} of {fmt.n(ev.lines_seen)} lines used, generated {fmt.stamp(ev.generated)}</span>
        </div>
        <div class="facts">
          {#if ev.envelope?.syslog}<div><span>envelope</span><b>syslog</b></div>{/if}
          {#if ev.envelope?.example_header}<div><span>header</span><b title={ev.envelope.example_header}>{ev.envelope.example_header}</b></div>{/if}
          {#if ev.params}
            <div><span>similarity</span><b>{ev.params.similarity}</b></div>
            <div><span>min support</span><b>{ev.params.min_support}</b></div>
            <div><span>rare share</span><b>{ev.params.rare_share}</b></div>
            <div><span>enum max</span><b>{ev.params.enum_max}</b></div>
            <div><span>max templates</span><b>{ev.params.max_templates}</b></div>
          {/if}
        </div>

        <div class="head quiet">
          <h3>Templates, {templates.length}</h3>
          <span class="note">keep or drop each, pick two or more to merge</span>
          <span class="push bar">
            {#if pickedIds.length > 1}
              <button class="btn primary" onclick={() => regenerate([pickedIds])} disabled={busy !== ''}>Merge {pickedIds.length} into one<kbd>m</kbd></button>
            {:else if pickedIds.length === 1}
              <span class="muted sm">pick a second template to merge</span>
            {/if}
            <button class="btn" onclick={() => regenerate([])} disabled={busy !== ''}>Regenerate from {keptIds.length} kept<kbd>r</kbd></button>
          </span>
        </div>

        {#each templates as t (t.id)}
          <article class="tpl" class:kept={keep[t.id]} class:out={!keep[t.id]} class:picked={picked[t.id]}>
            <header>
              <span class="id">#{t.id}</span>
              <span class="st">support <b>{fmt.n(t.support)}</b></span>
              <span class="st" class:is-warn={t.verified !== t.support && t.verified > 0} class:is-bad={t.verified === 0}>verified <b>{fmt.n(t.verified)}</b></span>
              <span class="st">members <b>{fmt.n(t.members?.length)}</b></span>
              {#if t.verified === 0}<span class="tag bad">left out of the definition</span>{/if}
              <span class="push bar">
                <label class="chk"><input type="checkbox" bind:checked={picked[t.id]} /> merge</label>
                <label class="chk"><input type="checkbox" bind:checked={keep[t.id]} /> keep</label>
              </span>
            </header>
            <pre class="pattern">{t.pattern}</pre>
            {#if t.slots?.length}
              <div class="wrap"><table class="tbl">
                <thead><tr><th>slot</th><th>kind</th><th>why this name</th><th>after</th><th class="num">distinct</th><th>examples</th></tr></thead>
                <tbody>
                  {#each t.slots as s}
                    <tr>
                      <td class="slot"><span class="slot-name" class:generic={!s.suggested}>{s.name}</span></td>
                      <td class="mono is-dim kind">{s.kind}</td>
                      <td class="reason">{s.reason ?? (s.suggested ? 'a rule produced this name; the server did not say which' : 'generic: no naming rule fired')}</td>
                      <td class="mono is-dim after" title={s.preceded_by}>{s.preceded_by}</td>
                      <td class="num">{fmt.n(s.distinct)}</td>
                      <td class="mono ex" title={(s.examples ?? []).join(' | ')}>{fmt.cut((s.examples ?? []).join('  '), 60)}</td>
                    </tr>
                  {/each}
                </tbody>
              </table></div>
            {/if}
            {#if t.examples?.length}
              <div class="ex">{#each t.examples as x}<pre>{x}</pre>{/each}</div>
            {/if}
            {#if t.history?.length}
              <div class="hist">{t.history.join('; ')}</div>
            {/if}
          </article>
        {:else}
          <div class="empty"><b>No templates in this proposal.</b><span>Every line fell below support or into a reason listed under unmatched.</span></div>
        {/each}

        <div class="head quiet"><h3>Unmatched, {fmt.n(ev.unmatched?.count ?? 0)}</h3><span class="note">lines no template covers, by reason</span></div>
        {#if ev.unmatched}
          <div class="bar">
            {#each Object.entries(ev.unmatched.by_reason ?? {}) as [k, v]}
              <span class="tag" class:warn={v > 0}>{k} {fmt.n(v)}</span>
            {/each}
          </div>
          {#if ev.unmatched.examples?.length}<div class="panel pad">{#each ev.unmatched.examples as x}<pre>{x}</pre>{/each}</div>{/if}
        {/if}

        <div class="head quiet"><h3>Decisions</h3><span class="note">every threshold the engine applied, in order</span></div>
        {#if ev.decisions?.length}
          <ol class="ol">{#each ev.decisions as d}<li>{d}</li>{/each}</ol>
        {:else}
          <div class="empty"><b>No decisions recorded.</b></div>
        {/if}
        <p class="xs muted">fingerprint <code>{ev.fingerprint}</code></p>
      </section>
    </div>
  {/if}
{/if}
