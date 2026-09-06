<script>
  // The deliberate step before an action that writes: approve, reject, replay, verify.
  // The key that opened it is never the key that confirms it (a letter opens, Enter
  // confirms), focus lands on the confirming button, Esc cancels, Tab reaches Cancel.
  // Enter and Escape stop here so the screen behind does not also act on them.
  let { title, hint = '', verb = 'Confirm', danger = false, onconfirm, oncancel, children } = $props()
  let btn = $state(null)
  let box = $state(null)
  // The footer is fixed over the page, so focus alone can leave the buttons under it:
  // scroll the whole box into view (its scroll-margin-bottom clears the footer).
  $effect(() => { btn?.focus(); box?.scrollIntoView({ block: 'nearest' }) })
  function key(e) {
    if (e.key === 'Escape') { e.stopPropagation(); e.preventDefault(); oncancel?.(); return }
    if (e.key === 'Enter') {
      e.stopPropagation()
      if (!(e.target instanceof HTMLButtonElement)) { e.preventDefault(); onconfirm?.() }
    }
  }
</script>

<div class="confirm" class:danger bind:this={box} role="alertdialog" aria-label={title} tabindex="-1" onkeydown={key}>
  <b>{title}</b>
  {#if children}<div class="what">{@render children()}</div>{/if}
  {#if hint}<p class="sm dim">{hint}</p>{/if}
  <div class="actions">
    <button class="btn primary" class:danger bind:this={btn} onclick={onconfirm}>{verb}<kbd>Enter</kbd></button>
    <button class="btn" onclick={oncancel}>Cancel<kbd>Esc</kbd></button>
    <span class="hint">Tab moves between the two</span>
  </div>
</div>
