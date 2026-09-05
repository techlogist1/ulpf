// Proves the console keeps painting under a full-rate drop: headless Chrome on Flow, a
// requestAnimationFrame monitor for --secs seconds, and --drop copied into the watched
// directory one second in, so the whole drop happens while the monitor runs. Then, with
// --trace, the time from navigation to the first painted row of that record's byte ruler.
//   node perf.mjs --base http://127.0.0.1:7891 --drop /tmp/l1/slice3.log --into /tmp/l1/watch/slice3.log [--secs 40] [--trace <raw id>]
import puppeteer from 'puppeteer-core'
import { copyFileSync } from 'node:fs'

const arg = (k, d) => { const i = process.argv.indexOf(`--${k}`); return i > 0 ? process.argv[i + 1] : d }
const base = arg('base', 'http://127.0.0.1:7891')
const chrome = arg('chrome', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')
const secs = Number(arg('secs', 40))
const drop = arg('drop', null)
const into = arg('into', null)
const trace = arg('trace', null)

const browser = await puppeteer.launch({ executablePath: chrome, headless: true })
const page = await browser.newPage()
await page.setViewport({ width: 1280, height: 800 })
await page.goto(`${base}/#/flow`, { waitUntil: 'domcontentloaded' })
await page.waitForFunction(() => document.querySelector('.flow .line'))
const before = await page.evaluate(() => Number(document.querySelector('.flow .station .num')?.textContent.replace(/,/g, '')))
await page.evaluate((secs) => {
  window.__mon = { frames: 0, over50: 0, over100: 0, worst: 0, tasks: 0, taskMax: 0, done: false }
  const start = performance.now()
  let last = start
  const tick = (t) => {
    const m = window.__mon
    m.frames++
    const gap = t - last
    if (gap > m.worst) m.worst = gap
    if (gap > 50) m.over50++
    if (gap > 100) m.over100++
    last = t
    if (t - start < secs * 1000) requestAnimationFrame(tick)
    else { m.done = true; m.secs = (t - start) / 1000 }
  }
  requestAnimationFrame(tick)
  new PerformanceObserver((l) => { for (const e of l.getEntries()) { window.__mon.tasks++; window.__mon.taskMax = Math.max(window.__mon.taskMax, e.duration) } }).observe({ entryTypes: ['longtask'] })
}, secs)
if (drop && into) setTimeout(() => copyFileSync(drop, into), 1000)
await page.waitForFunction(() => window.__mon.done, { timeout: (secs + 15) * 1000, polling: 500 })
const mon = await page.evaluate(() => window.__mon)
const after = await page.evaluate(() => Number(document.querySelector('.flow .station .num')?.textContent.replace(/,/g, '')))
const skipped = await page.evaluate(() => document.querySelector('.foot .push b')?.textContent)
console.log(`flow under load: ${after - before} events framed during ${mon.secs.toFixed(1)} s; ${mon.frames} animation frames = ${(mon.frames / mon.secs).toFixed(1)}/s; worst gap ${mon.worst.toFixed(0)} ms; gaps over 50 ms ${mon.over50}, over 100 ms ${mon.over100}; long tasks ${mon.tasks} (longest ${mon.taskMax.toFixed(0)} ms); status line frames skipped ${skipped}`)
if (trace != null) {
  const t0 = Date.now()
  await page.evaluate((id) => { location.hash = `#/trace/${id}` }, trace)
  await page.waitForFunction(() => document.querySelector('.bytes .vr'), { polling: 'raf', timeout: 60000 })
  const rows = await page.evaluate(() => document.querySelectorAll('.bytes .vr').length)
  const len = await page.evaluate(() => document.querySelector('.facts')?.textContent)
  console.log(`traceback ${trace}: first painted ruler row ${Date.now() - t0} ms after navigation, ${rows} rows in the DOM (${(len ?? '').replace(/\s+/g, ' ').trim().slice(0, 120)})`)
}
await browser.close()
