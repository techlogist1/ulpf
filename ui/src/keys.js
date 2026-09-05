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

// j/k or arrows move, Enter opens, g/G jump. Returns true when it handled the key.
// The virtual lists keep their own selected row in view, so nothing scrolls here.
export function nav(e, len, sel, set, open) {
  const move = (n) => { set(Math.max(0, Math.min(len - 1, n))); return true }
  if (!len) return false
  if (e.key === 'j' || e.key === 'ArrowDown') return move(sel < 0 ? 0 : sel + 1)
  if (e.key === 'k' || e.key === 'ArrowUp') return move(sel < 0 ? 0 : sel - 1)
  if (e.key === 'g') return move(0)
  if (e.key === 'G') return move(len - 1)
  if (e.key === 'Enter' && sel >= 0 && open) { open(sel); return true }
  return false
}

// Theme: dark by default (the operations context); `t` flips it, the choice is kept per browser.
export function theme(next) {
  let t = next
  if (!t) { try { t = localStorage.getItem('ulpf.theme') } catch { /* private window */ } }
  if (t === 'light') document.documentElement.dataset.theme = 'light'
  else delete document.documentElement.dataset.theme
  if (next) { try { localStorage.setItem('ulpf.theme', next) } catch { /* nothing to keep */ } }
  return document.documentElement.dataset.theme === 'light' ? 'light' : 'dark'
}
