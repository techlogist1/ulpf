<script>
  // A fixed-height-row window over `items`: only the rows in view (plus a margin) are in
  // the DOM, so a 500-row tail, a 30,000-row byte dump and a 100,000-entry diff cost the
  // same. `sel` is kept in view when it changes. `header` and `row` are snippets that
  // render one grid line each; the column template comes from --cols on the wrapper.
  let { items = [], max = 480, sel = -1, header = null, row, cls = '', rowH = 22 } = $props()
  let el = $state(null)
  let top = $state(0)
  const total = $derived(items.length * rowH)
  const height = $derived(Math.max(rowH, Math.min(max, total)))
  const start = $derived(Math.max(0, Math.floor(top / rowH) - 6))
  const end = $derived(Math.min(items.length, Math.ceil((top + height) / rowH) + 6))
  const slice = $derived(items.slice(start, end))
  $effect(() => {
    if (!el || sel < 0) return
    const y = sel * rowH
    if (y < el.scrollTop) el.scrollTop = y
    else if (y + rowH > el.scrollTop + el.clientHeight) el.scrollTop = y + rowH - el.clientHeight
  })
  export function scrollToTop() { if (el) el.scrollTop = 0 }
</script>

<div class={cls}>
  {#if header}{@render header()}{/if}
  <div class="vl" bind:this={el} style="height:{height}px" onscroll={() => (top = el.scrollTop)}>
    <div class="inner" style="height:{total}px">
      <div class="win" style="transform:translateY({start * rowH}px)">
        {#each slice as it, i (start + i)}{@render row(it, start + i)}{/each}
      </div>
    </div>
  </div>
</div>
