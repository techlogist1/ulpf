// The frame-budget rule, exercised: jank the main thread for five frames and Flow's pulses
// must stop with the note on screen; ten quiet seconds later they must run again. --base http://host:port
import puppeteer from 'puppeteer-core'
import { copyFileSync } from 'node:fs'
import { join } from 'node:path'
const arg = (k, d) => { const i = process.argv.indexOf(`--${k}`); return i > 0 ? process.argv[i + 1] : d }
const into = arg('into', null), sample = arg('sample', null) // a watch dir and one .log: the rate the pulses resume at
const browser = await puppeteer.launch({ executablePath: arg('chrome', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'), headless: true })
const page = await browser.newPage()
await page.goto(`${arg('base')}/#/flow`, { waitUntil: 'domcontentloaded' })
await page.waitForFunction(() => document.querySelectorAll('.flow .pulse').length > 0, { timeout: 15000 })
const state = () => page.evaluate(() => ({
  note: !!document.querySelector('.flow p.muted.sm'),
  rates: document.getAnimations().filter((a) => a.effect?.target?.classList?.contains('pulse')).map((a) => a.playbackRate),
}))
const results = []
const check = (name, ok, got) => results.push({ name, ok, got: JSON.stringify(got) })
check('no note while the machine keeps up', !(await state()).note, await state())
await page.evaluate(() => new Promise((res) => {
  let n = 0
  const step = () => { const t = performance.now(); while (performance.now() - t < 80) {} ; if (++n < 5) requestAnimationFrame(step); else res() }
  requestAnimationFrame(step)
}))
await new Promise((r) => setTimeout(r, 300))
let s = await state()
check('five janked frames pause the pulses', s.note && s.rates.length > 0 && s.rates.every((r) => r === 0), s)
await new Promise((r) => setTimeout(r, 11500))
// The pulses only run while events move, and the rate window is the last five frames, so keep
// dropping a sample in until one is running (or give up after eight).
if (into && sample) {
  for (let i = 0; i < 8 && !(await state()).rates.some((r) => r > 0); i++) {
    copyFileSync(sample, join(into, `budget-${Date.now()}.log`))
    await new Promise((r) => setTimeout(r, 700))
  }
}
s = await state()
check('ten quiet seconds clear it and the pulses run again', !s.note && (!(into && sample) || s.rates.some((r) => r > 0)), s)
for (const r of results) console.log(`${r.ok ? 'ok  ' : 'FAIL'} ${r.name}${r.ok ? '' : ` (got ${r.got})`}`)
const bad = results.filter((r) => !r.ok)
console.log(`${results.length - bad.length}/${results.length} budget ok`)
await browser.close(); process.exit(bad.length ? 1 : 0)
