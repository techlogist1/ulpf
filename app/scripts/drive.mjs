// The Windows measurement job's driver. A tester reported that the desktop app lags,
// crashes, ignores the number keys, will not let a parser be edited, and will not reset.
// Nobody has a Windows machine, so this measures those five things on the windows-latest
// runner instead of assuming them: OS-level keystrokes through the real input path,
// animation-frame gaps under a live drop, the review edit landing on disk, and reset.
//
//   node app/scripts/drive.mjs --cdp http://127.0.0.1:9222 --url-file <data>/server.url \
//        --data <data> --repo <checkout> --out <dir> --secs 60 --pid <ulpf-app.exe pid>
//   node app/scripts/drive.mjs --chrome <chrome> --url http://127.0.0.1:7893 ...   # local
//
// LOCAL MODE (--chrome) launches headless Chrome against a plain `ulpf serve`: no shell, so
// no OS keys and no reset. It exists to prove the phases that do not need Windows.
import { createRequire } from 'node:module'
import { pathToFileURL, fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join, basename } from 'node:path'

// puppeteer-core is a devDependency of ui/, not of app/. Resolving against ui/package.json
// lets this script live in app/scripts and run from any cwd.
const req = createRequire(new URL('../../ui/package.json', import.meta.url))
const mod = await import(pathToFileURL(req.resolve('puppeteer-core')).href)
const puppeteer = mod.default ?? mod
const OSKEYS = fileURLToPath(new URL('./oskeys.ps1', import.meta.url))

const argv = process.argv.slice(2)
const arg = (k, d) => { const i = argv.indexOf(`--${k}`); return i >= 0 ? argv[i + 1] : d }
const cdp = arg('cdp', null)
const chromePath = arg('chrome', null)
const urlFile = arg('url-file', null)
const fixedUrl = arg('url', null)
const data = arg('data', null)
const repo = arg('repo', null)
const out = arg('out', 'diagnostic')
const secs = Number(arg('secs', 60))
const appPid = arg('pid', null)
const deadlineMin = Number(arg('deadline-min', 12))
const mode = cdp ? 'cdp' : 'local'

if (!data || !repo) { console.error('--data and --repo are required'); process.exit(2) }
mkdirSync(out, { recursive: true })

const report = { mode, started: new Date().toISOString(), engine_url: null, phases: {}, findings: [], failures: [] }
const lines = []
const say = (s) => { lines.push(s); console.log(s) }

/// The report is the deliverable, so nothing may hang past the deadline without writing it:
/// a job-level timeout kills the runner before the artifact upload step ever runs.
function finish(code) {
  report.finished = new Date().toISOString()
  try { writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 2)) } catch { /* the disk answers or it does not */ }
  try { writeFileSync(join(out, 'summary.txt'), lines.join('\n') + '\n') } catch { /* same */ }
  process.exit(code)
}
const watchdog = setTimeout(() => {
  const m = `the driver passed its ${deadlineMin} minute deadline; the phases it finished are in report.json`
  report.failures.push(m)
  say(`::error::${m}`)
  finish(1)
}, deadlineMin * 60000)
watchdog.unref?.()

// ---- small helpers -------------------------------------------------------------------

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

/// Every wait is bounded and names its own reason, so a timeout is already the diagnosis.
async function poll(reason, ms, fn, step = 250) {
  const end = Date.now() + ms
  let last = null
  let err = ''
  for (;;) {
    try { last = await fn() } catch (e) { last = null; err = ` last error: ${e.message}` }
    if (last) return last
    if (Date.now() > end) throw new Error(`${reason} (waited ${ms} ms)${err}`)
    await sleep(step)
  }
}

function engineUrl() {
  if (urlFile) return readFileSync(urlFile, 'utf8').trim().replace(/\/$/, '')
  return (fixedUrl ?? '').replace(/\/$/, '')
}

async function getJSON(path) {
  const r = await fetch(engineUrl() + path, { signal: AbortSignal.timeout(5000) })
  if (!r.ok) throw new Error(`${path} answered ${r.status}`)
  return r.json()
}

function newPhase(name) {
  const p = { name, checks: [], info: {} }
  report.phases[name] = p
  return p
}
/// kind 'assert' decides the exit code; 'info' is printed and never fails the job.
function check(p, name, ok, detail, kind = 'assert') {
  p.checks.push({ name, ok: !!ok, kind, detail })
  if (!ok && kind === 'assert') report.failures.push(`${p.name}: ${name} — ${detail ?? ''}`)
  say(`  [${ok ? 'ok  ' : kind === 'assert' ? 'FAIL' : 'note'}] ${name}${detail ? ` — ${detail}` : ''}`)
  return ok
}

async function shot(page, name) {
  try { await page.screenshot({ path: join(out, `${name}.png`) }) } catch (e) { report.findings.push(`screenshot ${name}: ${e.message}`) }
}

// ---- the page ------------------------------------------------------------------------

let browser
try {
if (mode === 'cdp') {
  // WebView2 with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222.
  // browserURL discovery works on Chrome; WebView2 is reached through its websocket.
  // The endpoint is up once WebView2's environment exists, which is not the moment the
  // process started, so this is a bounded poll like every other wait here.
  browser = await poll(`nothing answered CDP on ${cdp}: WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS did not take`, 30000, async () => {
    try {
      return await puppeteer.connect({ browserURL: cdp, defaultViewport: null })
    } catch {
      const v = await (await fetch(`${cdp}/json/version`, { signal: AbortSignal.timeout(5000) })).json()
      return await puppeteer.connect({ browserWSEndpoint: v.webSocketDebuggerUrl, defaultViewport: null })
    }
  }, 1000)
} else {
  browser = await puppeteer.launch({ executablePath: chromePath, headless: true, args: ['--disable-gpu'] })
}
} catch (e) {
  report.failures.push(`no browser to drive: ${e.message}`)
  say(`::error::no browser to drive: ${e.message}`)
  finish(1)
}

// Capture-phase recorder: it separates "no keydown reached the document at all" (the OS or
// the webview ate it) from "the key arrived and the UI did not route it".
const RECORD = () => {
  if (window.__keysInstalled) return
  window.__keysInstalled = true
  window.__keys = []
  const rec = (e) => window.__keys.push({ type: e.type, key: e.key, target: (e.target && e.target.tagName) || '', ts: Date.now() })
  window.addEventListener('keydown', rec, true)
  window.addEventListener('keyup', rec, true)
}
// SSE counting has to be in place before the app connects, so it is a new-document script.
const SSE = () => {
  if (window.__sseInstalled) return
  window.__sseInstalled = true
  window.__sse = {}
  window.__sseBytes = 0
  const add = EventSource.prototype.addEventListener
  EventSource.prototype.addEventListener = function (type, fn, opts) {
    return add.call(this, type, (e) => { window.__sse[type] = (window.__sse[type] || 0) + 1; window.__sseBytes += e.data ? e.data.length : 0; return fn(e) }, opts)
  }
}

async function install(page) {
  await page.evaluateOnNewDocument(RECORD)
  await page.evaluateOnNewDocument(SSE)
  try { await page.evaluate(RECORD) } catch { /* mid-navigation; the new-document copy covers it */ }
}

/// Finds the page target serving the engine URL. On Windows the splash lives at
/// http://tauri.localhost/, so the engine page is the one whose URL starts with the URL the
/// shell wrote. Re-resolved after every navigation (reset, holder page).
async function enginePage(reason = 'the engine page', ms = 30000) {
  const page = await poll(`${reason} never appeared`, ms, async () => {
    const base = engineUrl()
    if (!base) return null
    for (const p of await browser.pages()) {
      try { if (p.url().startsWith(base)) return p } catch { /* target gone */ }
    }
    return null
  })
  await install(page)
  return page
}

/// The splash: URL host tauri.localhost (tauri://localhost on macOS), fragment as given.
async function splashPage(fragmentStarts, ms = 30000) {
  const page = await poll(`the splash page with a '${fragmentStarts}' fragment never appeared`, ms, async () => {
    for (const p of await browser.pages()) {
      const u = p.url()
      if (!/tauri(\.|:\/\/)localhost/.test(u)) continue
      const h = decodeURIComponent(u.split('#').slice(1).join('#'))
      if (h.startsWith(fragmentStarts)) return p
    }
    return null
  })
  return page
}

// A key that reloads, or the shell navigating, rejects an evaluate in flight. Every caller
// wants "what does the page look like now", never a thrown navigation, so the guard lives
// here once instead of in each of them: a lost sample is a retry, not a lost report.
const view = async (page) => {
  try {
    return await page.evaluate(() => ({
      hash: location.hash, overlay: !!document.querySelector('.overlay'),
      active: (document.activeElement && document.activeElement.tagName) || '',
      focus: document.hasFocus(), vis: document.visibilityState,
    }))
  } catch (e) {
    return { hash: '', overlay: false, active: '', focus: false, vis: '', error: e.message.slice(0, 120) }
  }
}

async function waitView(page, pred, ms = 1500) {
  const end = Date.now() + ms
  let v
  for (;;) {
    v = await view(page)
    if (pred(v)) return { ok: true, v }
    if (Date.now() > end) return { ok: false, v }
    await sleep(75)
  }
}

// ---- OS-level input (Windows only) ----------------------------------------------------

let shell = null
function pwsh() {
  if (shell) return shell
  for (const c of ['pwsh', 'powershell']) {
    if (spawnSync(c, ['-NoProfile', '-Command', 'exit 0'], { encoding: 'utf8' }).status === 0) { shell = c; return shell }
  }
  return null
}
function osrun(extra) {
  const sh = pwsh()
  if (!sh || !appPid) return { code: -1, out: '', err: 'no pwsh or no --pid' }
  // Bounded: a SendKeys that never returns would otherwise hold the event loop past the
  // deadline that writes the report. A timeout lands in the row as os_exit null / ETIMEDOUT.
  const r = spawnSync(sh, ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', OSKEYS, '-Pid', String(appPid), ...extra], { encoding: 'utf8', timeout: 30000, killSignal: 'SIGKILL' })
  return { code: r.status, out: (r.stdout ?? '').trim(), err: (r.error?.message ?? r.stderr ?? '').trim() }
}
const osKey = (k) => osrun(['-Keys', k])

// SendKeys names for the keys this driver sends, and their CDP equivalents for the retry.
const CDP_KEY = { '{ESC}': 'Escape', '{ENTER}': 'Enter', '^+r': null }

async function pressCdp(page, k) {
  const key = CDP_KEY[k] === undefined ? k : CDP_KEY[k]
  if (!key) return false
  await page.keyboard.press(key)
  return true
}

/// One key, one expectation, judged within 1.5 s, with what the document actually saw.
async function keyStep(page, k, label, pred, useOs) {
  await page.evaluate(() => { window.__keys = [] }).catch(() => {})
  const before = await view(page)
  let os = null
  if (useOs) os = osKey(k)
  else await pressCdp(page, k)
  let { ok, v } = await waitView(page, pred)
  const seen = await page.evaluate(() => window.__keys ?? []).catch(() => [])
  const row = {
    key: k, expected: label, ok, path: useOs ? 'os' : 'cdp',
    before: before.hash, after: v.hash, overlay: v.overlay, focus: v.focus, active: v.active,
    keydown: seen.some((x) => x.type === 'keydown'), keyup: seen.some((x) => x.type === 'keyup'),
    targets: [...new Set(seen.map((x) => x.target))].join(','),
    os_exit: os ? os.code : null, os_err: os ? os.err.slice(0, 200) : null,
  }
  // A text box that holds focus owns every key by the UI's own contract (ui/src/keys.js
  // typing()), so a key typed into one is the mild form of "the number keys do nothing". No
  // screen takes focus on its own (D101); if one does, it is named here rather than hidden,
  // Esc is the way out, and the key is sent once more to say whether the routing itself is
  // alive. The verdict stays the first result: a repair is a diagnosis, not a pass.
  if (!ok && /^(INPUT|TEXTAREA|SELECT)$/.test(v.active)) {
    row.focus_trap = v.active
    report.findings.push(`${k} was typed into the ${v.active} that ${v.hash} had focused instead of routing (ui/src/keys.js typing()): a screen took focus on its own, which D101 forbids. Esc is the way out; this is the mild form of "the number keys do nothing".`)
    if (useOs) osKey('{ESC}'); else await pressCdp(page, '{ESC}')
    await sleep(150)
    if (useOs) osKey(k); else await pressCdp(page, k)
    const r = await waitView(page, pred)
    row.recovered_with_esc = r.ok
    row.after = r.v.hash
    v = r.v
  }
  if (!ok && useOs) {
    // The diagnosis: the same key straight into the renderer. If this passes and the OS path
    // did not, the loss is in focus or WebView2's accelerator handling, not in the UI.
    await page.evaluate(() => { window.__keys = [] }).catch(() => {})
    const sent = await pressCdp(page, k)
    const r = sent ? await waitView(page, pred) : { ok: false, v: await view(page) }
    row.cdp_retry = { sent, ok: r.ok, after: r.v.hash, overlay: r.v.overlay }
  }
  return row
}

function digitRun(page, useOs) {
  const steps = [
    ['1', "hash '#/live'", (v) => v.hash === '#/live'],
    ['?', 'a .overlay element exists', (v) => v.overlay],
    ['{ESC}', 'no .overlay', (v) => !v.overlay],
    ['0', "hash '#/flow'", (v) => v.hash === '#/flow'],
    ['2', "hash '#/review'", (v) => v.hash === '#/review'],
    ['3', "hash '#/trace'", (v) => v.hash === '#/trace'],
    ['4', "hash '#/pivot'", (v) => v.hash === '#/pivot'],
    ['5', "hash '#/replay'", (v) => v.hash === '#/replay'],
    ['6', "hash '#/drift'", (v) => v.hash === '#/drift'],
    ['7', "hash '#/integrity'", (v) => v.hash === '#/integrity'],
    ['{ESC}', "back to '#/flow' from Integrity", (v) => v.hash === '#/flow'],
  ]
  return (async () => {
    const rows = []
    for (const [k, label, pred] of steps) rows.push(await keyStep(page, k, label, pred, useOs))
    return rows
  })()
}

// ---- phase (a): targets ----------------------------------------------------------------

const pa = newPhase('a_targets')
say(`ULPF Windows driver — mode ${mode}, secs ${secs}, data ${data}`)
let page
try {
  report.engine_url = engineUrl()
  if (mode === 'local') {
    page = await browser.newPage()
    await page.setViewport({ width: 1280, height: 820 })
    await install(page)
    await page.goto(`${engineUrl()}/#/flow`, { waitUntil: 'domcontentloaded' })
  } else {
    page = await enginePage()
  }
  await page.waitForFunction(() => document.querySelector('main .screen'), { timeout: 30000 })
  const v = await view(page)
  pa.info = { url: page.url(), ...v, targets: (await browser.pages()).map((p) => p.url()) }
  check(pa, 'the engine page is the one on screen', page.url().startsWith(engineUrl()), page.url())
  check(pa, 'the keydown recorder is installed', await page.evaluate(() => !!window.__keysInstalled), '', 'info')
  await shot(page, 'a-targets')
} catch (e) {
  check(pa, 'find the engine page', false, e.message)
  console.log(`::error::phase a: ${e.message}`)
  finish(1)
}

// ---- phase (b): keys, before and after a click ------------------------------------------

const pb = newPhase('b_keys')
const keyDetail = (r) => `after ${r.after}, keydown ${r.keydown}, target ${r.targets}${r.focus_trap ? `, ${r.focus_trap} held focus; routed after Esc: ${r.recovered_with_esc}` : ''}${r.cdp_retry ? `, cdp retry ${r.cdp_retry.ok ? 'passed' : 'failed'}` : ''}${r.os_exit === 3 ? ', the app window was not the foreground' : ''}`
try {
  if (mode === 'local') {
    say('phase b: local mode — page.keyboard only, no OS keystrokes and no focus question')
    pb.info.note = 'local mode: page.keyboard only, the OS path was not exercised'
    pb.info.before_click = await digitRun(page, false)
    for (const r of pb.info.before_click) check(pb, `cdp key ${r.key} → ${r.expected}`, r.ok, keyDetail(r))
    await shot(page, 'b-keys-cdp')
  } else {
    say('phase b: OS keystrokes on a fresh launch, before anything is clicked')
    pb.info.before_click = await digitRun(page, true)
    for (const r of pb.info.before_click) {
      // Before the click these are findings, not failures: a window that never took the
      // foreground is exactly the thing being measured, and the after-click run is the verdict.
      const ok = check(pb, `os key ${r.key} → ${r.expected} (before click)`, r.ok, keyDetail(r), 'info')
      if (!ok) report.findings.push(`before click: os key ${r.key} did not reach ${r.expected}; keydown seen=${r.keydown}; cdp retry ${r.cdp_retry?.ok ? 'passed (focus/accelerator loss)' : 'also failed (UI routing)'}`)
    }
    await shot(page, 'b-keys-before-click')

    const rect = osrun(['-Rect'])
    pb.info.window_rect = rect.out
    let clicked = false
    try {
      const r = JSON.parse(rect.out)
      // Screen coordinates: a point well inside the page, below the app's own chrome.
      const x = Math.round((r.left + r.right) / 2)
      const y = Math.round(r.top + (r.bottom - r.top) * 0.6)
      // One 'X,Y' argument: PowerShell binds `-Click 800 400` as 800 to -Click and 400 to the
      // next positional parameter, which would type "400" into the app and never click.
      const c = osrun(['-Click', `${x},${y}`])
      clicked = c.code === 0
      pb.info.click = { x, y, exit: c.code, out: c.out.slice(0, 200), err: c.err.slice(0, 200) }
    } catch (e) {
      pb.info.click = { error: `${e.message}; rect stderr ${rect.err.slice(0, 200)}` }
    }
    // An assertion, not a note: every after-click judgement below is void if no click landed,
    // and a silent no-click once made this whole run green against an untouched window.
    check(pb, 'an OS-level click landed inside the window', clicked, JSON.stringify(pb.info.click))

    page = await enginePage('the engine page after the click')
    await page.evaluate(() => { location.hash = '#/flow' }).catch(() => {})
    await sleep(400)
    pb.info.after_click = await digitRun(page, true)
    for (const r of pb.info.after_click) {
      check(pb, `os key ${r.key} → ${r.expected} (after click)`, r.ok, keyDetail(r))
    }
    await shot(page, 'b-keys-after-click')
  }
} catch (e) {
  check(pb, 'the key run completed', false, e.message)
}

// ---- phase (c): frame budget ------------------------------------------------------------

const pc = newPhase('c_frames')
const samples = existsSync(join(repo, 'samples')) ? readdirSync(join(repo, 'samples')).filter((f) => f.endsWith('.log')).map((f) => join(repo, 'samples', f)) : []
const watch = join(data, 'watch')

async function frameBudget(screen) {
  mkdirSync(watch, { recursive: true })
  // A reload, not a hash change: the SSE counter is a new-document script, so the document
  // has to execute once with it in place.
  await page.goto(`${engineUrl()}/#/${screen}`, { waitUntil: 'domcontentloaded' })
  await page.reload({ waitUntil: 'domcontentloaded' })
  await page.waitForFunction(() => document.querySelector('main .screen'), { timeout: 30000 })
  await page.evaluate((s) => {
    window.__mon = { frames: 0, over50: 0, over100: 0, worst: 0, tasks: 0, taskMax: 0, taskTotal: 0, done: false, gaps: [] }
    const start = performance.now()
    let last = start
    const tick = (t) => {
      const m = window.__mon
      m.frames++
      const gap = t - last
      if (m.frames > 1) { m.gaps.push(gap); if (gap > m.worst) m.worst = gap; if (gap > 50) m.over50++; if (gap > 100) m.over100++ }
      last = t
      if (t - start < s * 1000) requestAnimationFrame(tick)
      else { m.done = true; m.secs = (t - start) / 1000 }
    }
    requestAnimationFrame(tick)
    try {
      new PerformanceObserver((l) => { for (const e of l.getEntries()) { const m = window.__mon; m.tasks++; m.taskTotal += e.duration; m.taskMax = Math.max(m.taskMax, e.duration) } }).observe({ entryTypes: ['longtask'] })
    } catch { /* no longtask support */ }
  }, secs)

  let dropped = 0
  const timer = samples.length ? setInterval(() => {
    const f = samples[dropped % samples.length]
    try { copyFileSync(f, join(watch, `${basename(f, '.log')}-${Date.now()}.log`)); dropped++ } catch { /* the engine has the directory */ }
  }, 3000) : null
  try {
    await page.waitForFunction(() => window.__mon.done, { timeout: (secs + 30) * 1000, polling: 1000 })
  } finally {
    if (timer) clearInterval(timer)
  }

  const mon = await page.evaluate(() => window.__mon)
  const sse = await page.evaluate(() => ({ types: window.__sse ?? {}, bytes: window.__sseBytes ?? 0 }))
  const st = await view(page)
  let met = {}
  try { met = await page.metrics() } catch { /* not every CDP host implements it */ }
  const g = mon.gaps.slice().sort((a, b) => a - b)
  const pct = (q) => (g.length ? +g[Math.min(g.length - 1, Math.floor(g.length * q))].toFixed(1) : null)
  return {
    screen, secs: +mon.secs.toFixed(1), dropped, fps: +(mon.frames / mon.secs).toFixed(1),
    gap_ms: { p50: pct(0.5), p95: pct(0.95), p99: pct(0.99), worst: +mon.worst.toFixed(1), over50: mon.over50, over100: mon.over100 },
    long_tasks: { count: mon.tasks, longest_ms: +mon.taskMax.toFixed(0), total_ms: +mon.taskTotal.toFixed(0) },
    dom_nodes: met.Nodes ?? null, heap_mb: met.JSHeapUsedSize ? +(met.JSHeapUsedSize / 1048576).toFixed(1) : null,
    sse, has_focus: st.focus, visibility: st.vis,
  }
}

for (const screen of ['flow', 'live']) {
  try {
    const r = await frameBudget(screen)
    pc.info[screen] = r
    // Informational always: a GPU-less VM is not the tester's laptop. The numbers are the
    // point, not a verdict.
    say(`  ${screen}: ${r.fps} fps over ${r.secs} s, gaps p50 ${r.gap_ms.p50} p95 ${r.gap_ms.p95} p99 ${r.gap_ms.p99} worst ${r.gap_ms.worst} ms, over50 ${r.gap_ms.over50}, over100 ${r.gap_ms.over100}, long tasks ${r.long_tasks.count} (longest ${r.long_tasks.longest_ms} ms), dom ${r.dom_nodes}, heap ${r.heap_mb} MB, sse ${JSON.stringify(r.sse.types)} ${r.sse.bytes} B, focus ${r.has_focus}, ${r.visibility}, ${r.dropped} samples dropped in`)
    check(pc, `${screen} frame budget measured`, true, `${r.fps} fps, p99 gap ${r.gap_ms.p99} ms`, 'info')
    // fps on a GPU-less VM says nothing about the tester's laptop, but a long task and a
    // 100 ms frame gap are the app's own JS either way, so those two get named.
    if (r.long_tasks.longest_ms > 250) report.findings.push(`${screen}: a ${r.long_tasks.longest_ms} ms long task blocked the main thread (${r.long_tasks.count} tasks, ${r.long_tasks.total_ms} ms total)`)
    if (r.gap_ms.over100 > 5) report.findings.push(`${screen}: ${r.gap_ms.over100} frame gaps over 100 ms in ${r.secs} s (worst ${r.gap_ms.worst} ms)`)
  } catch (e) {
    check(pc, `${screen} frame budget measured`, false, e.message, 'info')
    report.findings.push(`frame budget ${screen}: ${e.message}`)
  }
  await shot(page, `c-frames-${screen}`)
}

// ---- phase (d): the review edit, end to end ----------------------------------------------

const pd = newPhase('d_review')
const useOs = mode === 'cdp'
try {
  mkdirSync(watch, { recursive: true })
  // The proposal's id and name come from the file's stem, so it must land as mikrotik.log.
  copyFileSync(join(repo, 'heldout', 'mikrotik.log'), join(watch, 'mikrotik.log'))
  const prop = await poll('GET /api/pending never listed a proposal for mikrotik.log', 40000, async () => {
    const list = await getJSON('/api/pending')
    return Array.isArray(list) ? list.find((p) => `${p.source} ${p.id}`.includes('mikrotik')) : null
  }, 1000)
  pd.info.proposal = prop
  check(pd, 'a proposal for mikrotik.log is listed', !!prop, `${prop.id}: ${prop.lines} lines, ${prop.templates} templates`)

  await page.goto(`${engineUrl()}/#/review/${encodeURIComponent(prop.id)}`, { waitUntil: 'domcontentloaded' })
  await page.waitForSelector('textarea.editor', { timeout: 20000 })
  const text = await page.$eval('textarea.editor', (el) => el.value)
  // The device's own name for the inbound interface. If the engine ever stops producing it,
  // rename the first slot the definition does declare instead.
  const slot = (n) => `{${n}:`
  let target = text.includes(slot('in_interface')) ? 'in_interface' : (text.match(/\{([A-Za-z_][\w-]*):[a-z]/) ?? [])[1]
  // A generated definition names its slots inline as {name:type}; a hand-shaped one names
  // them in a field table. Rename only those two forms: a blind string replace would rewrite
  // the same word inside a matcher literal or a pattern's constant tokens and corrupt it.
  if (!target) throw new Error('the definition declares no {name:type} slot to rename')
  const count = text.split(slot(target)).length - 1
  const edited = text.split(slot(target)).join(slot('in_if'))
  pd.info.rename = { from: target, to: 'in_if', occurrences: count }
  // Svelte 5's bind:value listens for 'input'; setting .value alone never reaches the state.
  await page.$eval('textarea.editor', (el, v) => { el.value = v; el.dispatchEvent(new Event('input', { bubbles: true })) }, edited)
  await page.evaluate(() => {
    const b = [...document.querySelectorAll('button')].find((x) => x.textContent.trim().startsWith('Save'))
    if (!b) throw new Error('no Save button on the review screen')
    b.click()
  })
  const saved = await poll('the review screen never said "Saved"', 20000, () =>
    page.evaluate(() => [...document.querySelectorAll('.notice b')].map((b) => b.textContent.trim()).find((t) => t.startsWith('Saved')) ?? null))
  // "Saved, N problems remain" also starts with Saved, and approve is gated on there being
  // none, so the exact word is the assertion: otherwise a corrupted rename fails three
  // checks later under the wrong name.
  check(pd, 'the Save button reported Saved with no problems', saved === 'Saved', saved)
  await shot(page, 'd-review-saved')

  const pendingFile = join(data, 'pending', `${prop.id}.toml`)
  const onDisk = existsSync(pendingFile) ? readFileSync(pendingFile, 'latin1') : null
  const crlf = onDisk ? (onDisk.match(/\r\n/g) ?? []).length : 0
  const lf = onDisk ? (onDisk.match(/\n/g) ?? []).length - crlf : 0
  pd.info.pending_file = { path: pendingFile, exists: !!onDisk, crlf, lf, bytes: onDisk?.length ?? 0 }
  check(pd, 'the edit is in the pending file on disk', !!onDisk && onDisk.includes('in_if'),
    onDisk ? `${pendingFile}: in_if ${onDisk.includes('in_if')}, ${crlf} CRLF, ${lf} bare LF` : `${pendingFile} does not exist`)
  say(`  line endings in ${basename(pendingFile)}: ${crlf} CRLF, ${lf} bare LF`)

  // Approve through the keyboard, because that is the path the tester used: 'a' opens the
  // confirmation, Enter confirms it.
  if (useOs) osKey('a'); else await page.keyboard.press('a')
  const asked = await poll('the approve confirmation never opened after "a"', 8000, () => page.evaluate(() => !!document.querySelector('.confirm')))
  check(pd, "'a' opened the approve confirmation", asked, mode)
  await shot(page, 'd-review-confirm')
  if (useOs) osKey('{ENTER}'); else await page.keyboard.press('Enter')

  const parsers = await poll('GET /api/parsers never listed mikrotik_inferred', 15000, async () => {
    const list = await getJSON('/api/parsers')
    return (Array.isArray(list) ? list : []).find((p) => JSON.stringify(p).includes('mikrotik_inferred')) ?? null
  }, 500)
  check(pd, 'GET /api/parsers lists mikrotik_inferred', !!parsers, JSON.stringify(parsers).slice(0, 200))

  const parserFile = join(data, 'parsers', 'mikrotik_inferred.toml')
  const def = existsSync(parserFile) ? readFileSync(parserFile, 'utf8') : null
  pd.info.parser_file = { path: parserFile, exists: !!def }
  check(pd, 'the approved parser is on disk with the edit and the generated marks', !!def && def.includes('in_if') && /origin\s*=\s*"inferred"/.test(def) && /priority\s*=\s*-1/.test(def),
    def ? `in_if ${def.includes('in_if')}, origin inferred ${/origin\s*=\s*"inferred"/.test(def)}, priority -1 ${/priority\s*=\s*-1/.test(def)}` : `${parserFile} does not exist`)
  await shot(page, 'd-review-approved')

  const again = join(watch, `mikrotik-again-${Date.now()}.log`)
  copyFileSync(join(repo, 'heldout', 'mikrotik.log'), again)
  const outFile = join(data, 'out.jsonl')
  const hit = await poll(`${outFile} never named mikrotik_inferred with in_if after ${basename(again)} was dropped`, 10000, () => {
    if (!existsSync(outFile)) return null
    const tail = readFileSync(outFile, 'utf8').trimEnd().split('\n').slice(-400)
    const named = tail.filter((l) => l.includes('"mikrotik_inferred"'))
    const carry = named.filter((l) => l.includes('"in_if"'))
    return carry.length ? { named: named.length, carry: carry.length, example: carry[carry.length - 1].slice(0, 300) } : null
  }, 500)
  pd.info.output = hit
  check(pd, 'the newest output lines name mikrotik_inferred and carry in_if', !!hit, `${hit.carry} of ${hit.named} newest mikrotik_inferred lines carry in_if`)
  say(`  review edit: ${target} → in_if in ${count} place(s); ${hit.carry} of the newest ${hit.named} mikrotik_inferred output lines carry in_if`)
} catch (e) {
  check(pd, 'the review edit reached the output', false, e.message)
}

// ---- phase (e): reset (Windows only) -----------------------------------------------------

// Taken here, not by the caller afterwards: "reset to first launch" deletes the data
// directory and every engine start truncates engine.log (src-tauri/src/reset.rs), so this is
// the last moment the log of the run just measured still exists.
try { copyFileSync(join(data, 'engine.log'), join(out, 'engine.log.measured-run')) } catch { /* the shell may not have written one */ }

const pe = newPhase('e_reset')
if (mode !== 'cdp') {
  pe.info.note = 'local mode: no shell, so no Reset menu and no splash'
  say('phase e: skipped — local mode has no shell to reset')
} else {
  /// Two ways to the same command. WebView2 keeps its own browser accelerators on by
  /// default and Ctrl+Shift+R is its hard reload, so it can consume the key before Tao's
  /// accelerator table sees it — on Windows only, which is why macOS never showed this.
  /// The menu is the path a user has left when that happens, and the report says which
  /// one worked instead of leaving a 30 s timeout to be read as "reset is broken".
  async function reachSplash(label) {
    const k = osKey('^+r')   // Ctrl+Shift+R, the Reset accelerator (app/src-tauri/src/menu.rs)
    try {
      return { splash: await splashPage('?', 12000), path: 'accelerator', accel_exit: k.code, accel_err: k.err.slice(0, 200) }
    } catch {
      report.findings.push(`${label}: Ctrl+Shift+R did not open the reset page (WebView2 keeps its own accelerators, and Ctrl+Shift+R is its hard reload); falling back to the File menu`)
      // One SendWait, not two calls: Alt+F opens the File menu, r picks "Reset…" (the only
      // item there starting with R), and a second process would re-take the foreground and
      // close the menu before the letter arrived.
      const m = osKey('%fr')
      return { splash: await splashPage('?', 20000), path: 'file menu', accel_exit: k.code, accel_err: `${k.err.slice(0, 120)} | menu ${m.code}: ${m.err.slice(0, 120)}` }
    }
  }

  async function doReset(button, label) {
    const t0 = Date.now()
    const before = engineUrl()
    const k = await reachSplash(label)
    const splash = k.splash
    check(pe, `${label}: the reset page opened from the keyboard`, true, `via the ${k.path}`, 'info')
    const body = await splash.evaluate(() => document.body.innerText)
    const buttons = await splash.evaluate(() => [...document.querySelectorAll('#choices button')].map((b) => b.id))
    check(pe, `${label}: the reset page names the data directory`, body.includes(data), body.split('\n').slice(0, 3).join(' | ').slice(0, 240))
    check(pe, `${label}: the three reset buttons exist`, buttons.length === 3, buttons.join(','))
    await shot(splash, `e-splash-${button}`)
    await splash.click(`#${button}`)

    // The engine comes back on a fresh port, so the URL file is the only truth.
    await poll(`${label}: server.url never changed and the old engine kept answering`, 40000, async () => {
      if (engineUrl() && engineUrl() !== before) return true
      try { await fetch(`${before}/api/status`, { signal: AbortSignal.timeout(1500) }); return false } catch { return true }
    }, 500)
    const p = await enginePage(`${label}: the engine page after the reset`, 40000)
    await p.waitForFunction(() => document.querySelector('main .screen'), { timeout: 40000 })
    await poll(`${label}: the Flow screen never showed a counter`, 20000, () => p.evaluate(() => !!document.querySelector('.station .num')))
    const ms = Date.now() - t0
    return { page: p, ms, path: k.path, accel_exit: k.accel_exit, accel_err: k.accel_err }
  }

  const dirCount = (d, f = () => true) => (existsSync(d) ? readdirSync(d).filter(f).length : 0)
  try {
    const r1 = await doReset('reset-keep', 'reset events')
    page = r1.page
    await page.evaluate(() => { location.hash = '#/flow' })
    await sleep(500)
    // The first station is `framed`; it reads '–' until the first metrics frame lands, so
    // wait for a number before judging it.
    const framed = await poll('the Flow screen never showed a framed counter after the reset', 15000, async () => {
      const t = await page.evaluate(() => document.querySelector('.station .num')?.textContent?.trim() ?? null)
      return t && t !== '–' ? t : null
    }, 500)
    pe.info.keep = { ms: r1.ms, path: r1.path, framed, parsers: dirCount(join(data, 'parsers')), watch: dirCount(watch), out: existsSync(join(data, 'out.jsonl')) ? readFileSync(join(data, 'out.jsonl'), 'utf8').length : -1 }
    check(pe, 'reset events: the approved parser survived', existsSync(join(data, 'parsers', 'mikrotik_inferred.toml')), join(data, 'parsers', 'mikrotik_inferred.toml'))
    check(pe, 'reset events: watch/ is empty', dirCount(watch) === 0, `${dirCount(watch)} entries`)
    check(pe, 'reset events: out.jsonl is empty or absent', pe.info.keep.out <= 0, `${pe.info.keep.out} bytes`)
    check(pe, 'reset events: Flow shows 0 framed', framed === '0', `Flow reads ${framed}`)
    const k1 = await keyStep(page, '1', "hash '#/live'", (v) => v.hash === '#/live', true)
    pe.info.keep.key1 = k1
    check(pe, 'reset events: the window still takes an OS key', k1.ok, `after ${k1.after}, keydown ${k1.keydown}`)
    say(`  reset events: ready Flow ${r1.ms} ms after the reset was asked for via the ${r1.path}, ${pe.info.keep.parsers} parsers kept, Flow reads ${framed}`)
    await shot(page, 'e-after-reset-keep')

    const r2 = await doReset('reset-all', 'reset to first launch')
    page = r2.page
    const inferred = existsSync(join(data, 'parsers')) ? readdirSync(join(data, 'parsers')).filter((f) => f.includes('_inferred')) : []
    // The bundle carries this checkout's parsers/*.toml (tauri.conf.json resources), so the
    // expected count is read from the checkout rather than frozen at the 15 of the day.
    const bundled = dirCount(join(repo, 'parsers'), (f) => f.endsWith('.toml'))
    pe.info.all = { ms: r2.ms, path: r2.path, parsers: dirCount(join(data, 'parsers'), (f) => f.endsWith('.toml')), bundled, inferred, watch: dirCount(watch) }
    check(pe, `first launch: the ${bundled} bundled parsers came back`, pe.info.all.parsers === bundled, `${pe.info.all.parsers} *.toml in ${join(data, 'parsers')} against ${bundled} in ${join(repo, 'parsers')}`)
    check(pe, 'first launch: no generated parser came back', inferred.length === 0, inferred.join(',') || 'none')
    check(pe, 'first launch: watch/ is empty', pe.info.all.watch === 0, `${pe.info.all.watch} entries`)
    say(`  reset to first launch: ready Flow ${r2.ms} ms after the reset was asked for via the ${r2.path}, ${pe.info.all.parsers} parsers, inferred ${inferred.length}`)
    await shot(page, 'e-after-reset-all')
  } catch (e) {
    check(pe, 'reset ran from the keyboard and came back up', false, e.message)
  }
}

// ---- phase (f): the verdict --------------------------------------------------------------

// (b)'s before-click run is informational; the after-click run, (d) and (e) decide.
const failures = report.failures
say('')
say(`summary: ${Object.keys(report.phases).length} phases, ${failures.length} failing assertion(s), ${report.findings.length} finding(s)`)
for (const f of report.findings) say(`  finding: ${f}`)

if (mode === 'local') { try { await browser.close() } catch { /* already gone */ } } else { try { browser.disconnect() } catch { /* already gone */ } }
// Findings are annotations, not the verdict: a frame gap on a GPU-less VM must not red the
// job, but it must be visible in the run's own summary without opening the artifact.
for (const f of report.findings) console.log(`::warning::${f}`)
for (const f of failures) console.log(`::error::${f}`)
finish(failures.length ? 1 : 0)
