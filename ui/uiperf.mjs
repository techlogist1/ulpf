// Frame budget of the served UI under a "sample every few seconds" session, in headless
// Chrome (the same engine WebView2 renders with). Per screen: animation-frame gaps, long
// tasks, DOM node count, JS heap, and the SSE events the page received by type.
//   node uiperf.mjs --base http://127.0.0.1:PORT --screen flow --secs 60 --drop-every 3 --into <watch dir> --samples <dir with *.log>
import puppeteer from 'puppeteer-core'
import { copyFileSync, readdirSync } from 'node:fs'
import { join, basename } from 'node:path'

const arg = (k, d) => { const i = process.argv.indexOf(`--${k}`); return i > 0 ? process.argv[i + 1] : d }
const base = arg('base')
const chrome = arg('chrome', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')
const secs = Number(arg('secs', 60))
const screen = arg('screen', 'flow')
const every = Number(arg('drop-every', 3))
const into = arg('into', null)
const samples = arg('samples', null)
const width = Number(arg('width', 1280)), height = Number(arg('height', 820))

const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ['--disable-gpu'] })
const page = await browser.newPage()
await page.setViewport({ width, height })
// Count SSE events by type before the app connects: wrap addEventListener on EventSource.
await page.evaluateOnNewDocument(() => {
  window.__sse = {}
  const add = EventSource.prototype.addEventListener
  EventSource.prototype.addEventListener = function (type, fn, opts) {
    return add.call(this, type, (e) => { window.__sse[type] = (window.__sse[type] || 0) + 1; window.__sseBytes = (window.__sseBytes || 0) + (e.data ? e.data.length : 0); return fn(e) }, opts)
  }
})
await page.goto(`${base}/#/${screen}`, { waitUntil: 'domcontentloaded' })
await page.waitForFunction(() => document.querySelector('main .screen'))
await page.evaluate((secs) => {
  window.__mon = { frames: 0, over50: 0, over100: 0, worst: 0, tasks: 0, taskMax: 0, taskTotal: 0, done: false, gaps: [] }
  const start = performance.now()
  let last = start
  const tick = (t) => {
    const m = window.__mon
    m.frames++
    const gap = t - last
    if (m.frames > 1) { m.gaps.push(gap); if (gap > m.worst) m.worst = gap; if (gap > 50) m.over50++; if (gap > 100) m.over100++ }
    last = t
    if (t - start < secs * 1000) requestAnimationFrame(tick)
    else { m.done = true; m.secs = (t - start) / 1000 }
  }
  requestAnimationFrame(tick)
  new PerformanceObserver((l) => { for (const e of l.getEntries()) { const m = window.__mon; m.tasks++; m.taskTotal += e.duration; m.taskMax = Math.max(m.taskMax, e.duration) } }).observe({ entryTypes: ['longtask'] })
}, secs)

const files = samples ? readdirSync(samples).filter((f) => f.endsWith('.log')).map((f) => join(samples, f)) : []
let dropped = 0
let timer = null
if (into && files.length) {
  timer = setInterval(() => {
    const f = files[dropped % files.length]
    copyFileSync(f, join(into, `${basename(f, '.log')}-${Date.now()}.log`))
    dropped++
  }, every * 1000)
}
const mid = []
const sampler = setInterval(async () => {
  try {
    const met = await page.metrics()
    mid.push({ t: Math.round(met.Timestamp), nodes: met.Nodes, heapMB: +(met.JSHeapUsedSize / 1048576).toFixed(1) })
  } catch { /* page busy */ }
}, 10000)
await page.waitForFunction(() => window.__mon.done, { timeout: (secs + 30) * 1000, polling: 1000 })
clearInterval(timer); clearInterval(sampler)
const mon = await page.evaluate(() => window.__mon)
const sse = await page.evaluate(() => ({ types: window.__sse, bytes: window.__sseBytes || 0 }))
const met = await page.metrics()
const gaps = mon.gaps.sort((a, b) => a - b)
const pct = (p) => gaps.length ? gaps[Math.min(gaps.length - 1, Math.floor(gaps.length * p))].toFixed(1) : '–'
const counters = await page.evaluate(() => {
  const t = (sel) => document.querySelector(sel)?.textContent?.trim()
  return { framed: t('.flow .station .num') || t('.funnel .fst .num'), skipped: t('.foot .push b'), evicted: [...document.querySelectorAll('.foot span')].find((s) => s.textContent.includes('events skipped'))?.querySelector('b')?.textContent }
})
console.log(JSON.stringify({
  screen, secs: +mon.secs.toFixed(1), dropped, fps: +(mon.frames / mon.secs).toFixed(1),
  gap_ms: { p50: +pct(0.5), p95: +pct(0.95), p99: +pct(0.99), worst: +mon.worst.toFixed(1), over50: mon.over50, over100: mon.over100 },
  long_tasks: { count: mon.tasks, longest_ms: +mon.taskMax.toFixed(0), total_ms: +mon.taskTotal.toFixed(0) },
  dom_nodes: met.Nodes, heap_mb: +(met.JSHeapUsedSize / 1048576).toFixed(1), samples: mid,
  sse, counters,
}))
await browser.close()
