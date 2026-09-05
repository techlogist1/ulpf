// Captures every screen of a running `ulpf serve` headlessly, at 1280x800 and 2560x1440,
// plus the stateful shots (hover, hex, overlay, empty, error, the keyboard-only approve
// flow). Re-run: node capture.mjs --base http://127.0.0.1:7881 --out ../docs/screens
//   [--trace <raw id>] [--big <raw id of the multi-megabyte record>] [--approve <pending id>] [--update <pending id>] [--pivot kind=value]
//   [--empty <base of a second server with zero events>]
// Writes README.md beside the PNGs, one line per capture. Needs Chrome on this machine.
import puppeteer from 'puppeteer-core'
import { mkdirSync, writeFileSync, existsSync } from 'node:fs'
import { join } from 'node:path'

const arg = (k, d) => { const i = process.argv.indexOf(`--${k}`); return i > 0 ? process.argv[i + 1] : d }
const base = arg('base', 'http://127.0.0.1:7881')
const out = arg('out', '../docs/screens')
const chrome = arg('chrome', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')
// --only a,b,c re-shoots those names (approve = the five-step flow) and leaves README.md and index.json alone.
const only = arg('only', null)?.split(',') ?? null
const wanted = (name) => !only || only.includes(name)
mkdirSync(out, { recursive: true })

const j = async (p) => (await fetch(base + p)).json()
const status = await j('/api/status')
const tail = await j('/api/tail?limit=500')
const events = tail.events ?? []
const pick = (pred) => events.find(pred)?.raw_id
// A record every parser field lights: the Check Point sample has the most pairs.
const traceId = arg('trace', null) ?? pick((e) => e.line?.ulpf?.parser === 'check_point') ?? pick((e) => e.line?.ulpf?.parser) ?? events[0]?.raw_id ?? 0
const bigId = arg('big', null)
let pivotArg = arg('pivot', null)
if (!pivotArg) { const ent = (await j('/api/entities?limit=1')).entities?.[0]; if (ent) pivotArg = `${ent.kind}=${ent.value}` }
const [pk, pv] = (pivotArg ?? 'src_ip=').split('=')
const pending = await j('/api/pending')
const approveId = arg('approve', null) ?? pending.find((p) => !p.updates)?.id ?? pending[0]?.id
const reviewId = pending.find((p) => p.templates > 3 && !p.updates)?.id ?? pending[0]?.id
const updateId = arg('update', null) ?? pending.find((p) => p.updates)?.id
const replay = await j('/api/replay')
const missing = String((await j('/api/integrity')).records + 100000)

const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ['--hide-scrollbars'] })
const index = []
async function shot(name, width, height, url, what, prepare) {
  if (!wanted(name)) return
  const page = await browser.newPage()
  await page.setViewport({ width, height, deviceScaleFactor: 1 })
  await page.goto(`${base}/${url}`, { waitUntil: 'domcontentloaded' })
  await page.waitForFunction(() => document.querySelector('main')?.children.length > 0)
  await new Promise((r) => setTimeout(r, 1400)) // two metrics frames, one tail frame
  if (prepare) await prepare(page)
  const file = `${name}-${width}.png`
  await page.screenshot({ path: join(out, file) })
  await page.evaluate(() => { try { localStorage.removeItem('ulpf.theme') } catch {} })
  index.push({ file, screen: name.replace(/-.*/, ''), width, what })
  await page.close()
  console.log(file)
}
const key = async (page, k, wait = 350) => { await page.keyboard.press(k); await new Promise((r) => setTimeout(r, wait)) }

for (const width of [1280, 2560]) {
  const height = width === 1280 ? 800 : 1440
  await shot('live', width, height, '#/live', 'live feed: rates, funnel, queue, tail, sources, parsers, every engine counter')
  await shot('review-list', width, height, '#/review', 'review: the pending proposals, kind, lines, templates, unmatched, problems')
  await shot('review-detail', width, height, `#/review/${encodeURIComponent(reviewId)}`, 'review: definition editor, actions, evidence with templates, slot names and the reason for each')
  if (updateId) await shot('review-update', width, height, `#/review/${encodeURIComponent(updateId)}`, 'review: a drift update proposal, the unified diff against the parser on disk above the definition and the evidence')
  await shot('trace', width, height, `#/trace/${traceId}`, 'traceback: verdicts, the byte ruler with every field lit, parser fields and normalized provenance')
  await shot('trace-hover', width, height, `#/trace/${traceId}`, 'traceback: j walks the normalized fields, the selected field is lit in the bytes and the parser fields', async (p) => { await key(p, 'j'); await key(p, 'j'); await key(p, 'j'); await key(p, 'Enter') })
  await shot('trace-hex', width, height, `#/trace/${traceId}`, 'traceback: the same record in hex, sixteen bytes per row, the lit field carried into the hex and ascii columns', async (p) => { await key(p, 'j'); await key(p, 'j'); await key(p, 'Enter'); await key(p, 'h') })
  if (bigId != null) await shot('trace-big', width, height, `#/trace/${bigId}`, 'traceback of the 4 MB single-line record: the byte ruler virtualises the text, the page stays responsive', async (p) => { const el = await p.$('.bytes .vl'); await el.evaluate((e) => { e.scrollTop = e.scrollHeight / 3 }); await new Promise((r) => setTimeout(r, 300)) })
  await shot('pivot-search', width, height, '#/pivot', 'pivot: kind selector and the entities with the most events')
  await shot('pivot', width, height, `#/pivot/${encodeURIComponent(pk)}/${encodeURIComponent(pv)}`, `pivot of the busiest entity (${pk} ${pv}): device lanes on a time axis, the timeline, the related entities`)
  await shot('replay', width, height, '#/replay', `replay: why v${replay.last?.version ?? '?'} differs, counters, parser changes, by field, versions, the diff entries`)
  await shot('drift', width, height, '#/drift', 'drift: every established source with its window rate against the baseline; tripped and proposed first')
  await shot('integrity', width, height, '#/integrity', 'integrity: verdict of the last verify, records, store id, genesis and chain head')
}
await shot('keys', 1280, 800, '#/live', 'the shortcut overlay (?)', async (p) => { await key(p, 'Shift'); await p.keyboard.type('?'); await new Promise((r) => setTimeout(r, 300)) })
await shot('empty-trace', 1280, 800, '#/trace', 'empty state: traceback with no record chosen')
await shot('error-trace', 1280, 800, `#/trace/${missing}`, `error state: a trace of raw id ${missing}, which the store never issued`)
await shot('light-trace', 1280, 800, `#/trace/${traceId}`, 'the same traceback under the light theme (t)', async (p) => { await key(p, 't') })
await shot('light-live', 1280, 800, '#/live', 'the live feed under the light theme (t)', async (p) => { await key(p, 't') })
await shot('empty-pivot-value', 1280, 800, '#/pivot/user/nobody-has-this-name', 'empty state: a pivot on a value no event carries')
await shot('reject-confirm', 1280, 800, `#/review/${encodeURIComponent(reviewId)}`, 'review: x opens the reject confirmation, marked as the destructive one; Enter confirms, Esc cancels', async (p) => { await key(p, 'x') })

// Keyboard-only approve: open review, walk to the proposal, open it, a, Enter.
if (approveId && wanted('approve')) {
  const page = await browser.newPage()
  await page.setViewport({ width: 1280, height: 800 })
  await page.goto(`${base}/#/live`, { waitUntil: 'domcontentloaded' })
  await page.waitForFunction(() => document.querySelector('main')?.children.length > 0)
  await new Promise((r) => setTimeout(r, 1200))
  const step = async (n, what) => { const file = `approve-${n}-1280.png`; await page.screenshot({ path: join(out, file) }); index.push({ file, screen: 'review', width: 1280, what }); console.log(file) }
  await key(page, '2', 600)
  await step(1, 'keyboard approve 1: the digit 2 opens Review from anywhere')
  const list = await j('/api/pending')
  const at = list.findIndex((p) => p.id === approveId)
  for (let i = 0; i <= at; i++) await key(page, 'j', 120)
  await step(2, `keyboard approve 2: j selects the proposal (${approveId})`)
  await key(page, 'Enter', 900)
  await step(3, 'keyboard approve 3: Enter opens it; the definition, the actions and the evidence')
  await key(page, 'a', 400)
  await step(4, 'keyboard approve 4: a opens the confirmation; focus is on Approve, Esc would cancel')
  await key(page, 'Enter', 1500)
  await step(5, 'keyboard approve 5: Enter confirms; the result names the file, the parsers loaded and how many buffered lines the new parser now claims')
  await page.close()
}
// A second, fresh server with zero events (--empty <base>): every screen's empty state.
const empty = arg('empty', null)
if (empty) {
  for (const [route, what] of [['live', 'a fresh server with zero events: rates, funnel, queue and the tail say what will fill them'], ['review', 'nothing to review: what makes a proposal appear'], ['pivot', 'no entities indexed yet'], ['replay', 'no output versions yet'], ['drift', 'no source established yet: the thresholds in words'], ['integrity', 'an empty store: the genesis is fixed, the head appears with the first record']]) {
    const page = await browser.newPage()
    await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 })
    await page.goto(`${empty}/#/${route}`, { waitUntil: 'domcontentloaded' })
    await page.waitForFunction(() => document.querySelector('main')?.children.length > 0)
    await new Promise((r) => setTimeout(r, 1400))
    const file = `empty-${route}-1280.png`
    await page.screenshot({ path: join(out, file) })
    index.push({ file, screen: route, width: 1280, what: `empty state: ${what}` })
    await page.close()
    console.log(file)
  }
}
await browser.close()
if (only) { console.log(`${index.length} captures re-shot in ${out}; README.md and index.json untouched`); process.exit(0) }

const lines = ['# Screen captures', '', `Captured headlessly by \`ui/capture.mjs\` against a populated \`ulpf serve\` (${status.version ?? 'ulpf'} at ${base}). One line per file.`, '', '| file | screen | width | what it shows |', '|---|---|---|---|', ...index.map((i) => `| ${i.file} | ${i.screen} | ${i.width} | ${i.what} |`)]
writeFileSync(join(out, 'README.md'), lines.join('\n') + '\n')
writeFileSync(join(out, 'index.json'), JSON.stringify(index, null, 2))
console.log(`${index.length} captures in ${out}`)
