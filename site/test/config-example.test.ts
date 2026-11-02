// Run with `pnpm test` (node --test). Node strips the types itself.
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { configExample, settings, yaml, type Widget } from '../src/lib/config-example.ts'

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repo = resolve(site, '..')

const widget = (over: Partial<Widget>): Widget => ({
  id: 'w',
  name: 'W',
  version: '1.0.0',
  description: '',
  tags: [],
  kind: 'builtin',
  source: 'builtin',
  default_size: 'small',
  config_schema: null,
  requirements: { binaries: [], env: [], hosts: [] },
  demo: { status: 'ok' },
  ...over,
})

test('yaml quotes strings YAML would read as something else', () => {
  for (const s of ['true', 'No', 'null', 'off', 'y', '~', '.inf', '', 'a: b', 'key:', '12', '-x', 'a,b', '#x']) {
    assert.equal(yaml(s), JSON.stringify(s), s)
  }
  for (const s of ['main', 'origin/main', 'https://example.com/x', '~/code', '@me']) assert.equal(yaml(s), s, s)
})

test('yaml writes numbers, lists and maps in flow style', () => {
  assert.equal(yaml(30), '30')
  assert.equal(yaml(false), 'false')
  assert.equal(yaml(['a', 'yes', 2]), '[a, "yes", 2]')
  assert.equal(yaml({ a: 1, b: 'x y' }), '{ a: 1, b: "x y" }')
})

test('settings follow $ref and list enum choices', () => {
  const w = widget({
    config_schema: {
      required: ['units'],
      properties: {
        units: { $ref: '#/$defs/Units' },
        sort: { allOf: [{ $ref: '#/$defs/Sort' }], default: 'name' },
        repos: { type: 'array', items: { type: 'string' }, default: [] },
        limit: { type: ['integer', 'null'], description: 'At most this many.' },
      },
      $defs: {
        Units: { enum: ['metric', 'imperial'], description: 'Which units.' },
        Sort: { oneOf: [{ const: 'name' }, { const: 'date' }] },
      },
    },
  })
  assert.deepEqual(settings(w), [
    { key: 'units', type: 'metric | imperial', required: true, default: '', description: 'Which units.' },
    { key: 'sort', type: 'name | date', required: false, default: 'name', description: '' },
    { key: 'repos', type: 'list of string', required: false, default: '[]', description: '' },
    { key: 'limit', type: 'integer', required: false, default: '', description: 'At most this many.' },
  ])
})

test('a widget with no settings still gets a module block with a value', () => {
  const example = configExample(widget({ id: 'cpu' }))
  assert.match(example, /^ {2}cpu:\n {4}demo: false/m)
})

// Every widget page's example, through the dashboard's own config check. Needs
// a built dashboard (cargo build -p dashboard, or DASHBOARD_BIN).
const bin = [process.env.DASHBOARD_BIN, join(repo, 'target/debug/dashboard'), join(repo, 'target/release/dashboard')]
  .filter((p): p is string => !!p)
  .find((p) => existsSync(p))

test('every config example loads in `dashboard config`', { skip: !bin && 'no dashboard binary' }, () => {
  const entries = JSON.parse(readFileSync(join(site, 'src/data/registry.json'), 'utf8')).entries as Widget[]
  const dir = join(repo, '.tmp', 'site-config-check')
  rmSync(dir, { recursive: true, force: true })
  // Packages are found in plugins/ next to the config; XDG_* keep this machine's out.
  mkdirSync(join(dir, 'plugins'), { recursive: true })
  for (const w of entries) {
    if (w.kind === 'plugin' && w.source !== 'bundled') symlinkSync(join(repo, w.source), join(dir, 'plugins', w.id))
  }
  const env = { ...process.env }
  for (const d of ['CONFIG', 'DATA', 'STATE', 'CACHE']) env[`XDG_${d}_HOME`] = join(dir, 'home', d.toLowerCase())
  const failed: string[] = []
  try {
    for (const w of entries) {
      const file = join(dir, `${w.id}.yaml`)
      writeFileSync(file, configExample(w) + '\n')
      try {
        execFileSync(bin!, ['--no-project', '--config', file, 'config'], { env, cwd: dir, stdio: 'pipe' })
      } catch (err: any) {
        failed.push(`${w.id}: ${String(err.stderr || err.stdout || err.message).trim()}`)
      }
    }
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
  assert.deepEqual(failed, [])
})
