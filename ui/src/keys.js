// One window listener lives in App; the screen on show registers the keys it owns.
let current = null

export function keys(handler) {
  current = handler
  return () => { if (current === handler) current = null }
}

export function screenKey(e) {
  return current ? current(e) === true : false
}

export function typing(e) {
  const t = e.target
  return t instanceof HTMLElement && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable)
}

// j/k or arrows move, Enter opens, Home/End jump. Returns true when it handled the key.
export function nav(e, len, sel, set, open) {
  const move = (n) => {
    const i = Math.max(0, Math.min(len - 1, n))
    set(i)
    requestAnimationFrame(() => document.querySelector('tr.sel, li.sel')?.scrollIntoView({ block: 'nearest' }))
    return true
  }
  if (!len) return false
  if (e.key === 'j' || e.key === 'ArrowDown') return move(sel < 0 ? 0 : sel + 1)
  if (e.key === 'k' || e.key === 'ArrowUp') return move(sel < 0 ? 0 : sel - 1)
  if (e.key === 'g') return move(0)
  if (e.key === 'G') return move(len - 1)
  if (e.key === 'Enter' && sel >= 0 && open) { open(sel); return true }
  return false
}
