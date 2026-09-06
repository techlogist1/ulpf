// The frame-budget rule, exercised (D102): with events moving, five janked frames must stop
// Flow's pulses with the note on screen and keep them stopped while events keep arriving;
// ten quiet seconds later they must run again. Events have to move for the check to mean
// anything (an idle pulse is at rate 0 with or without the rule), so a watch directory and
// one sample are required.
//   node budget.mjs --base http://host:port --into <watch dir> --sample <one .log>
import puppeteer from 'puppeteer-core'
import { copyFileSync } from 'node:fs'
import { join } from 'node:path'
const arg = (k, d) => { const i = process.argv.indexOf(`--${k}`); return i > 0 ? process.argv[i + 1] : d }
const into = arg('into'), sample = arg('sample')
if (!into || !sample) { console.error('budget.mjs: --into <watch dir> and --sample <file> are required'); process.exit(2) }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const browser = await puppeteer.launch({ executablePath: arg('chrome', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'), headless: true })
const page = await browser.newPage()
await page.goto(`${arg('base')}/#/flow`, { waitUntil: 'domcontentloaded' })
await page.waitForFunction(() => document.querySelectorAll('.flow .pulse').length > 0, { timeout: 15000 })
const state = () => page.evaluate(() => ({
  note: !!document.querySelector('.flow p.muted.sm'),
  rates: document.getAnimations().filter((a) => a.effect?.target?.classList?.contains('pulse')).map((a) => a.playbackRate),
  framed: Number((document.querySelector('.flow .station .num')?.textContent || '0').replace(/[^0-9]/g, '')),
}))
let n = 0
const drop = () => copyFileSync(sample, join(into, `budget-${Date.now()}-${n++}.log`))
// The state every 50 ms for `ms`, with a sample dropped every 400 ms: what the pulses did
// while events moved, summarised as polls, framed before and after, polls with a running
// pulse, polls with the note up.
const watch = async (ms) => {
  const polls = []
  const end = Date.now() + ms
  let next = 0
  while (Date.now() < end) {
    if (Date.now() >= next) { drop(); next = Date.now() + 400 }
    polls.push(await state())
    await sleep(50)
  }
  return { polls: polls.length, framed: [polls[0].framed, polls[polls.length - 1].framed], running: polls.filter((s) => s.rates.some((r) => r > 0)).length, noted: polls.filter((s) => s.note).length }
}
const results = []
const check = (name, ok, got) => results.push({ name, ok, got: JSON.stringify(got) })
let s = await watch(2500)
check('the pulses run while events move, no note', s.framed[1] > s.framed[0] && s.running > 0 && s.noted === 0, s)
await page.evaluate(() => new Promise((res) => {
  let n = 0
  const step = () => { const t = performance.now(); while (performance.now() - t < 80) {} ; if (++n < 5) requestAnimationFrame(step); else res() }
  requestAnimationFrame(step)
}))
await sleep(300)
s = await watch(2500)
check('five janked frames stop every pulse for as long as events keep moving, with the note', s.framed[1] > s.framed[0] && s.noted === s.polls && s.running === 0, s)
await sleep(10500)
s = await watch(2500)
check('ten quiet seconds clear it and the pulses run again', s.noted === 0 && s.running > 0, s)
for (const r of results) console.log(`${r.ok ? 'ok  ' : 'FAIL'} ${r.name}${r.ok ? '' : ` (got ${r.got})`}`)
const bad = results.filter((r) => !r.ok)
console.log(`${results.length - bad.length}/${results.length} budget ok`)
await browser.close(); process.exit(bad.length ? 1 : 0)
