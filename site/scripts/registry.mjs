// Builds the site's widget registry from the dashboard's own catalog, and the
// building blocks from examples/blocks/. Run from site/ (needs the dashboard
// binary and web/dist, see below):
//
//   node scripts/registry.mjs           # all four steps below
//   node scripts/registry.mjs data      # the catalog files in src/data/ only
//   node scripts/registry.mjs shots     # web pictures only
//   node scripts/registry.mjs tui       # terminal captures only
//   node scripts/registry.mjs app       # the embedded web app only
//
// Each step does two sets: the widgets and the building blocks (SET picks one).
//
// data: `dashboard widgets --json` for each config in CONFIGS, merged by ID.
// That is every built-in and bundled plugin, plus the packages next to those
// configs (plugins/ext and examples/plugins). XDG_* point into an empty folder
// so packages installed on this machine are left out. The blocks: each
// examples/blocks/<id>.json (an example view, checked by the contract's tests)
// shaped like a catalog entry of kind `block`, into src/data/blocks.json, and
// the view's JSON Schema (`dashboard schema view`) into
// src/data/view-schema.json, for the blocks' field tables.
//
// shots: serves web/dist and the catalog, opens each entry's demo view in
// headless Chrome (the web app's ?mode=demo page) at every tile size in every
// theme, and saves the tile as public/registry/<theme>/<id>-<size>.webp
// (blocks: public/block-shots/). Each picture's size in CSS pixels goes in
// src/data/registry-shots.json (blocks: block-shots.json). The directory cards
// use the default size's picture. Talks to Chrome over the DevTools protocol
// with Node's own WebSocket, so it needs no packages.
//
// tui: the TUI's tile_shots test (crates/tui/src/tile_shots.rs) draws each
// demo view as one tile at every size with the real tile renderer, laid out
// for a 120-column terminal, into src/data/tui-tiles/<theme>/<id>.json (cells
// with their colors; blocks: src/data/block-tiles/). Needs cargo.
//
// app: builds the web app (web/, needs its node_modules) into public/demo-app/
// with relative links, plus public/demo-app/catalog/<id>.json, one entry each
// (blocks: public/demo-app/blocks/<id>.json). Widget and block pages embed its
// demo page (?mode=demo&fill) in a frame the reader resizes. Committed like
// the rest, because the site's CI builds only site/.
//
// Settings, all optional: DASHBOARD_BIN (default ../target/debug/dashboard,
// then ../target/release/dashboard), CHROME (the Chrome or Chromium program),
// ONLY (comma-separated IDs: redoes just those, and without a step argument
// implies `shots` and `tui`), SET (`widgets` or `blocks`: just that one),
// THEMES (comma-separated, default dark,light), QUALITY (webp quality,
// default 40) and SCALE (pixels per CSS pixel, default 1.5).
//
// Needs Node 22 or later (for the global WebSocket). `pnpm test` checks that
// every widget page's config example loads in `dashboard config`.

import { execFileSync, spawn } from 'node:child_process'
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { dirname, extname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repo = resolve(site, '..')
const scratch = join(repo, '.tmp', 'site-registry')
const appDir = join(site, 'public/demo-app')
const blocksDir = join(repo, 'examples/blocks')
const schemaFile = join(site, 'src/data/view-schema.json')

/**
 * The two sets of tiles, each drawn the same way. `data`: the catalog file;
 * `shots`: the web pictures; `sizes`: each picture's size in CSS pixels (the
 * files are SCALE times that), for <img> width and height; `tui`: the
 * terminal captures; `app`: the embedded app's folder of one-entry catalogs.
 */
const ALL_SETS = [
  {
    name: 'widgets',
    data: join(site, 'src/data/registry.json'),
    shots: join(site, 'public/registry'),
    sizes: join(site, 'src/data/registry-shots.json'),
    tui: join(site, 'src/data/tui-tiles'),
    app: 'catalog',
  },
  {
    name: 'blocks',
    data: join(site, 'src/data/blocks.json'),
    shots: join(site, 'public/block-shots'),
    sizes: join(site, 'src/data/block-shots.json'),
    tui: join(site, 'src/data/block-tiles'),
    app: 'blocks',
  },
]
const SETS = ALL_SETS.filter((s) => !process.env.SET || s.name === process.env.SET)

/** Tile sizes, as the contract spells them (crates/contract/src/lib.rs, `Size`). */
const SIZES = ['small', 'medium', 'large', 'wide', 'tall']
const THEMES = (process.env.THEMES || 'dark,light').split(',').filter(Boolean)
const QUALITY = Number(process.env.QUALITY || 40)
const SCALE = Number(process.env.SCALE || 1.5)

/** Configs whose neighbouring plugins/ folders hold the packages to list. */
const CONFIGS = ['examples/plugins.yaml', 'examples/widgets/w-public.yaml']

const what = process.argv[2] ?? 'all'
if (!['all', 'data', 'shots', 'tui', 'app'].includes(what)) {
  console.error('usage: node scripts/registry.mjs [all|data|shots|tui|app]')
  process.exit(2)
}
if (!SETS.length) fail('SET: use widgets or blocks')
const only = process.env.ONLY?.split(',').filter(Boolean)

// ONLY redoes pictures; the catalog is always the whole thing.
if (what === 'data' || (what === 'all' && !only)) {
  for (const set of SETS) (set.name === 'widgets' ? writeData : writeBlocks)(set)
}
for (const set of SETS) {
  const entries = JSON.parse(readFileSync(set.data, 'utf8')).entries
  // With ONLY, a set without any of those IDs is left alone.
  if (only && !entries.some((e) => only.includes(e.id))) continue
  if (what === 'all' || what === 'shots') {
    // shots() throws instead of exiting, so its cleanup (Chrome, the server) runs first.
    await shots(set).catch((err) => fail(err.message))
  }
  if (what === 'all' || what === 'tui') tuiTiles(set)
}
if (what === 'app' || (what === 'all' && !only)) demoApp()

function writeData({ data: dataFile }) {
  const bin = dashboardBin()
  const home = join(scratch, 'home')
  rmSync(home, { recursive: true, force: true })
  const env = { ...process.env }
  for (const dir of ['CONFIG', 'DATA', 'STATE', 'CACHE']) env[`XDG_${dir}_HOME`] = join(home, dir.toLowerCase())
  const byId = new Map()
  for (const config of CONFIGS) {
    const out = execFileSync(bin, ['--config', join(repo, config), 'widgets', '--json'], { env, cwd: repo })
    for (const entry of JSON.parse(out.toString()).entries) {
      if (byId.has(entry.id)) continue
      // A package's folder, relative to the repository, not this machine.
      if (entry.source.startsWith('/')) entry.source = relative(repo, entry.source)
      byId.set(entry.id, entry)
    }
  }
  const entries = [...byId.values()]
  const bundles = bundleOf()
  for (const entry of entries) {
    if (entry.kind !== 'builtin') continue
    entry.bundle = bundles.get(entry.id)
    if (!entry.bundle) fail(`${entry.id}: not found in any crates/modules-<bundle> crate`)
  }
  writeFileSync(dataFile, JSON.stringify({ entries }, null, 1) + '\n')
  console.log(`${relative(site, dataFile)}: ${entries.length} widgets`)
}

/** examples/blocks/*.json as catalog entries, and the view's JSON Schema. */
function writeBlocks({ data: dataFile }) {
  const entries = readdirSync(blocksDir)
    .filter((f) => f.endsWith('.json'))
    .sort()
    .map((f) => {
      const b = JSON.parse(readFileSync(join(blocksDir, f), 'utf8'))
      // The catalog entry's fields, so the demo app and the TUI capture draw it like a widget.
      return {
        id: b.id,
        name: b.title,
        version: '',
        description: b.description,
        tags: [],
        kind: 'block',
        source: relative(repo, join(blocksDir, f)),
        default_size: b.default_size,
        requirements: { binaries: [], env: [], hosts: [] },
        demo: b.view,
        group: b.group,
      }
    })
  writeFileSync(dataFile, JSON.stringify({ entries }, null, 1) + '\n')
  console.log(`${relative(site, dataFile)}: ${entries.length} blocks`)
  const schema = execFileSync(dashboardBin(), ['schema', 'view'], { cwd: repo })
  writeFileSync(schemaFile, JSON.stringify(JSON.parse(schema.toString())) + '\n')
  console.log(relative(site, schemaFile))
}

/** Built-in module ID to its bundle (system, dev, ...): the crates/modules-<bundle> crate that declares it. */
function bundleOf() {
  const ids = new Map()
  const crates = join(repo, 'crates')
  for (const dir of readdirSync(crates)) {
    const bundle = dir.match(/^modules-(.+)$/)?.[1]
    if (!bundle || bundle === 'common') continue
    for (const file of readdirSync(join(crates, dir, 'src'), { recursive: true })) {
      if (!file.endsWith('.rs')) continue
      const text = readFileSync(join(crates, dir, 'src', file), 'utf8')
      for (const m of text.matchAll(/(?:\bid: |const ID: &str = )"([a-z0-9-]+)"/g)) ids.set(m[1], bundle)
    }
  }
  return ids
}

/** Builds the web app into public/demo-app/, with one catalog file per widget and per block. */
function demoApp() {
  const web = join(repo, 'web')
  execFileSync('pnpm', ['exec', 'vite', 'build', '--base', './', '--outDir', appDir, '--emptyOutDir'], {
    cwd: web,
    stdio: ['ignore', 'ignore', 'inherit'],
  })
  // The build empties the folder, so every set is written again.
  const counts = []
  for (const set of ALL_SETS) {
    const dir = join(appDir, set.app)
    mkdirSync(dir, { recursive: true })
    const entries = JSON.parse(readFileSync(set.data, 'utf8')).entries
    for (const entry of entries) writeFileSync(join(dir, `${entry.id}.json`), JSON.stringify({ entries: [entry] }))
    counts.push(`${entries.length} in ${set.app}/`)
  }
  console.log(`${relative(site, appDir)}: the web app and catalog files, ${counts.join(', ')}`)
}

/** Runs the TUI's tile_shots test, which writes the set's terminal captures. */
function tuiTiles({ data, tui }) {
  const env = { ...process.env, CATALOG: data, OUT: tui, THEMES: THEMES.join(','), DASHBOARD_SKIP_UI_BUILD: '1' }
  execFileSync('cargo', ['test', '-q', '-p', 'dashboard-tui', 'tile_shots', '--', '--ignored', '--nocapture'], {
    cwd: repo,
    env,
    stdio: ['ignore', 'inherit', 'inherit'],
  })
}

function dashboardBin() {
  const candidates = [process.env.DASHBOARD_BIN, '../target/debug/dashboard', '../target/release/dashboard']
  const found = candidates.filter(Boolean).map((p) => resolve(site, p)).find((p) => existsSync(p))
  if (!found) fail('no dashboard binary: run `cargo build -p dashboard` or set DASHBOARD_BIN')
  return found
}

function chromeBin() {
  const candidates = [
    process.env.CHROME,
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
    '/usr/bin/google-chrome',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
  ]
  const found = candidates.filter(Boolean).find((p) => existsSync(p))
  if (!found) fail('no Chrome found: set CHROME to a Chrome or Chromium program')
  return found
}

function fail(message) {
  console.error(message)
  process.exit(1)
}

// ---- screenshots ----

async function shots({ data: dataFile, shots: shotDir, sizes: sizeFile }) {
  const dist = join(repo, 'web/dist')
  if (!existsSync(join(dist, 'index.html'))) fail('no web/dist: run `pnpm build` in web/')
  const catalog = readFileSync(dataFile)
  const entries = JSON.parse(catalog.toString()).entries.filter((e) => !only || only.includes(e.id))
  // A full run starts clean, so removed widgets and themes lose their pictures.
  if (!only) rmSync(shotDir, { recursive: true, force: true })
  for (const theme of THEMES) mkdirSync(join(shotDir, theme), { recursive: true })
  // Kept across runs, so ONLY updates just its own entries.
  const kept = only && existsSync(sizeFile) ? JSON.parse(readFileSync(sizeFile, 'utf8')) : {}
  const sizes = kept.shots ?? {}
  const themes = [...new Set([...(kept.themes ?? []), ...THEMES])]

  const chromeProgram = chromeBin()
  const server = await serve(dist, catalog)
  const port = server.address().port
  const profile = join(scratch, 'chrome')
  rmSync(profile, { recursive: true, force: true })
  const chrome = spawn(
    chromeProgram,
    [
      '--headless=new',
      '--remote-debugging-port=0',
      `--user-data-dir=${profile}`,
      '--no-first-run',
      '--no-default-browser-check',
      '--hide-scrollbars',
      '--force-prefers-reduced-motion',
      '--window-size=1280,1000',
      'about:blank',
    ],
    { stdio: ['ignore', 'ignore', 'pipe'] },
  )
  try {
    const cdp = await connect(await devtoolsUrl(chrome))
    const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' })
    const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true })
    const page = (method, params) => cdp.send(method, params, sessionId)
    await page('Page.enable')
    await page('Emulation.setDeviceMetricsOverride', { width: 1280, height: 1000, deviceScaleFactor: SCALE, mobile: false })
    for (const theme of THEMES) {
      for (const entry of entries) {
        for (const size of SIZES) {
          const query = new URLSearchParams({ mode: 'demo', catalog: '/catalog.json', theme, id: entry.id, size })
          const url = `http://127.0.0.1:${port}/?${query}`
          await page('Page.navigate', { url })
          // The URL check skips the last page, still showing until this one loads.
          const ready = await waitFor(page, `location.href === ${JSON.stringify(url)} && document.documentElement.dataset.ready`)
          if (ready !== 'ok') throw new Error(`${entry.id} ${size} ${theme}: the demo page did not draw the tile`)
          // A moment for charts to measure themselves.
          await new Promise((r) => setTimeout(r, 150))
          const rect = await evaluate(page, `JSON.stringify(document.querySelector('.tile').getBoundingClientRect())`)
          const { x, y, width, height } = JSON.parse(rect)
          const shot = await page('Page.captureScreenshot', {
            format: 'webp',
            quality: QUALITY,
            captureBeyondViewport: true,
            clip: { x, y, width, height, scale: 1 },
          })
          const file = join(shotDir, theme, `${entry.id}-${size}.webp`)
          writeFileSync(file, Buffer.from(shot.data, 'base64'))
          // The same in every theme; the last one wins.
          sizes[entry.id] = { ...sizes[entry.id], [size]: [Math.round(width), Math.round(height)] }
          console.log(`${relative(site, file)}  ${sizes[entry.id][size].join('x')}`)
        }
      }
    }
    // One line per widget: {"themes": [...], "shots": {id: {size: [width, height]}}}.
    const lines = Object.entries(sizes)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([id, s]) => `  ${JSON.stringify(id)}: ${JSON.stringify(s)}`)
    writeFileSync(sizeFile, `{"themes": ${JSON.stringify(themes)}, "shots": {\n${lines.join(',\n')}\n}}\n`)
    cdp.close()
  } finally {
    if (chrome.exitCode === null && chrome.signalCode === null) {
      const exited = new Promise((r) => chrome.once('exit', r))
      chrome.kill()
      await exited
    }
    server.close()
    rmSync(profile, { recursive: true, force: true })
  }
}

/** Serves web/dist, plus the catalog at /catalog.json. */
function serve(dist, catalog) {
  const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.woff2': 'font/woff2' }
  const server = createServer((req, res) => {
    const path = new URL(req.url, 'http://x').pathname
    if (path === '/catalog.json') {
      res.writeHead(200, { 'content-type': 'application/json' }).end(catalog)
      return
    }
    const file = join(dist, path === '/' ? 'index.html' : path)
    if (!file.startsWith(dist) || !existsSync(file)) {
      res.writeHead(404).end()
      return
    }
    res.writeHead(200, { 'content-type': types[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file))
  })
  return new Promise((r) => server.listen(0, '127.0.0.1', () => r(server)))
}

/** The browser's DevTools WebSocket URL, which Chrome prints on stderr. */
function devtoolsUrl(chrome) {
  return new Promise((resolveUrl, reject) => {
    let text = ''
    const timer = setTimeout(() => reject(new Error(`Chrome did not start:\n${text}`)), 15000)
    chrome.stderr.on('data', (chunk) => {
      text += chunk
      const match = text.match(/DevTools listening on (ws:\S+)/)
      if (match) {
        clearTimeout(timer)
        resolveUrl(match[1])
      }
    })
    chrome.on('exit', () => reject(new Error(`Chrome exited:\n${text}`)))
  })
}

/** A minimal DevTools protocol client: send a command, await its result. */
async function connect(url) {
  const ws = new WebSocket(url)
  await new Promise((r, reject) => {
    ws.onopen = r
    ws.onerror = reject
  })
  let next = 0
  const waiting = new Map()
  ws.onmessage = (event) => {
    const msg = JSON.parse(event.data)
    const pending = msg.id && waiting.get(msg.id)
    if (!pending) return
    waiting.delete(msg.id)
    if (msg.error) pending.reject(new Error(`${pending.method}: ${msg.error.message} ${msg.error.data ?? ''}`))
    else pending.resolve(msg.result)
  }
  return {
    send(method, params = {}, sessionId) {
      const id = ++next
      ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }))
      return new Promise((resolveResult, reject) => waiting.set(id, { method, resolve: resolveResult, reject }))
    },
    close: () => ws.close(),
  }
}

async function evaluate(page, expression) {
  const { result } = await page('Runtime.evaluate', { expression, returnByValue: true })
  return result.value
}

/** Polls `expression` until it has a value, for up to ten seconds. */
async function waitFor(page, expression) {
  for (let i = 0; i < 200; i++) {
    const value = await evaluate(page, expression)
    if (value) return value
    await new Promise((r) => setTimeout(r, 50))
  }
  return undefined
}
