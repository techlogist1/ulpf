// The keyboard map, pressed through the browser (CDP), checked by effect. --base http://host:port
import puppeteer from 'puppeteer-core'
const arg = (k, d) => { const i = process.argv.indexOf(`--${k}`); return i > 0 ? process.argv[i + 1] : d }
const base = arg('base'); const chrome = arg('chrome', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')
const browser = await puppeteer.launch({ executablePath: chrome, headless: true })
const page = await browser.newPage(); await page.setViewport({ width: 1280, height: 820 })
await page.goto(`${base}/#/flow`, { waitUntil: 'domcontentloaded' }); await page.waitForFunction(() => document.querySelector('main .screen'))
const hash = () => page.evaluate(() => location.hash)
const overlay = () => page.evaluate(() => !!document.querySelector('.overlay .keymap'))
const theme = () => page.evaluate(() => document.documentElement.dataset.theme ?? 'dark')
const active = () => page.evaluate(() => document.activeElement?.tagName + (document.activeElement?.className ? '.' + document.activeElement.className : ''))
const results = []
const check = (name, ok, got) => results.push({ name, ok, got })
for (const [k, view] of [['1', 'live'], ['2', 'review'], ['3', 'trace'], ['4', 'pivot'], ['5', 'replay'], ['6', 'drift'], ['7', 'integrity'], ['0', 'flow']]) {
  if (k === '3') await page.keyboard.press('Escape') // trace with no id focuses its box; leave it before the next digit
  await page.keyboard.press(k); await new Promise((r) => setTimeout(r, 150))
  const h = await hash(); check(`digit ${k} -> #/${view}`, h.startsWith(`#/${view}`), h)
  if (k === '3') { const a = await active(); check('trace with no id focuses the raw-id box', a.startsWith('INPUT'), a); await page.keyboard.press('Escape') }
}
await page.keyboard.press('?'); await new Promise((r) => setTimeout(r, 150)); check('? opens the overlay', await overlay(), await active())
await page.keyboard.press('Escape'); await new Promise((r) => setTimeout(r, 150)); check('overlay open, Esc closes it', !(await overlay()), await active())
await page.keyboard.press('?'); await new Promise((r) => setTimeout(r, 100)); await page.keyboard.press('?'); await new Promise((r) => setTimeout(r, 100)); check('? again closes the overlay', !(await overlay()), '')
// Keys are never modal: with the map open every other key closes it and still does its job.
await page.keyboard.press('?'); await new Promise((r) => setTimeout(r, 150))
await page.keyboard.press('3'); await new Promise((r) => setTimeout(r, 200))
check('overlay open, 3 closes it and opens Traceback', !(await overlay()) && (await hash()).startsWith('#/trace'), `${await hash()} overlay=${await overlay()}`)
await page.keyboard.press('Escape'); await new Promise((r) => setTimeout(r, 100)) // leave the raw-id box
await page.keyboard.press('?'); await new Promise((r) => setTimeout(r, 150))
await page.keyboard.press('t'); await new Promise((r) => setTimeout(r, 150))
check('overlay open, t closes it and flips the theme', !(await overlay()) && (await theme()) === 'light', `${await theme()} overlay=${await overlay()}`)
await page.keyboard.press('t'); await new Promise((r) => setTimeout(r, 100)) // back to dark for the checks below
await page.keyboard.press('t'); await new Promise((r) => setTimeout(r, 100)); check('t -> light', (await theme()) === 'light', await theme())
await page.keyboard.press('t'); await new Promise((r) => setTimeout(r, 100)); check('t -> dark', (await theme()) === 'dark', await theme())
// The keys the overlay itself needs stay its own: Tab reaches the close button, Enter presses it.
await page.keyboard.press('?'); await new Promise((r) => setTimeout(r, 150))
await page.keyboard.press('Tab'); await new Promise((r) => setTimeout(r, 100))
const closeBtn = await active()
check('overlay open, Tab reaches the close button', (await overlay()) && closeBtn.startsWith('BUTTON'), `${closeBtn} overlay=${await overlay()}`)
await page.keyboard.press('Enter'); await new Promise((r) => setTimeout(r, 150))
check('the close button closes the overlay', !(await overlay()), await active())
await page.keyboard.press('1'); await new Promise((r) => setTimeout(r, 200))
await page.keyboard.press('Slash'); await new Promise((r) => setTimeout(r, 100)); check('/ focuses the Live filter', (await active()).startsWith('INPUT'), await active())
await page.keyboard.type('12'); await new Promise((r) => setTimeout(r, 100)); check('digits typed into the filter stay there', (await hash()) === '#/live' && (await page.evaluate(() => document.activeElement.value)) === '12', await page.evaluate(() => document.activeElement.value))
await page.keyboard.press('Escape'); await new Promise((r) => setTimeout(r, 100)); check('Esc leaves the filter', !(await active()).startsWith('INPUT'), await active())
await page.keyboard.press('f'); await new Promise((r) => setTimeout(r, 100)); check('f toggles flagged-only', await page.evaluate(() => document.querySelector('.bar .btn.on')?.textContent?.startsWith('Flagged')), '')
await page.keyboard.press('f')
await page.keyboard.press('e'); await new Promise((r) => setTimeout(r, 100)); check('e opens the export choice', !!(await page.evaluate(() => document.querySelector('.export'))), '')
await page.keyboard.press('Escape'); await new Promise((r) => setTimeout(r, 100)); check('Esc closes the export choice', !(await page.evaluate(() => document.querySelector('.export'))), '')
await page.keyboard.press('Space'); await new Promise((r) => setTimeout(r, 100)); check('space holds the tail', await page.evaluate(() => [...document.querySelectorAll('.bar .btn')].some((b) => b.textContent.startsWith('Release'))), '')
await page.keyboard.press('Space')
const rows = await page.evaluate(() => document.querySelectorAll('.tail .vr').length)
if (rows) {
  await page.keyboard.press('j'); await new Promise((r) => setTimeout(r, 100)); check('j selects a tail row', !!(await page.evaluate(() => document.querySelector('.tail .vr.sel'))), '')
  await page.keyboard.press('Enter'); await new Promise((r) => setTimeout(r, 300)); check('Enter traces the selected row', (await hash()).startsWith('#/trace/'), await hash())
  await page.waitForFunction(() => document.querySelector('.bytes .vr'), { timeout: 10000 })
  await page.keyboard.press('h'); await new Promise((r) => setTimeout(r, 150)); check('h switches the ruler to hex', !!(await page.evaluate(() => document.querySelector('.bytes.hexmode'))), '')
  await page.keyboard.press('j'); await new Promise((r) => setTimeout(r, 100)); check('j walks the normalized fields', !!(await page.evaluate(() => document.querySelector('.prov .vr.sel'))), '')
  await page.keyboard.press('Escape'); await new Promise((r) => setTimeout(r, 200)); check('Esc from a detail goes to Flow', (await hash()) === '#/flow', await hash())
} else check('tail has rows (needed for j/Enter/h)', false, '0 rows')
const bad = results.filter((r) => !r.ok)
for (const r of results) console.log(`${r.ok ? 'ok  ' : 'FAIL'} ${r.name}${r.ok ? '' : ` (got ${r.got})`}`)
console.log(`${results.length - bad.length}/${results.length} keys ok`)
await browser.close(); process.exit(bad.length ? 1 : 0)
