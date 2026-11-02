// Run with `pnpm test` (node --test). Node strips the types itself.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { AREAS, areaOf, sections } from '../src/lib/areas.ts'
import type { Widget } from '../src/lib/config-example.ts'

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const widgets: Widget[] = JSON.parse(readFileSync(join(site, 'src/data/registry.json'), 'utf8')).entries

test('every built-in is listed under its bundle', () => {
  for (const w of widgets.filter((w) => w.kind === 'builtin')) {
    assert.ok(w.bundle, `${w.id} has no bundle; rerun \`pnpm registry data\``)
    assert.equal(areaOf(w), w.bundle, w.id)
  }
})

test('every widget is in exactly one section', () => {
  const listed = [...sections(widgets, 'builtin'), ...sections(widgets, 'plugin')].flatMap((s) => s.widgets)
  assert.equal(listed.length, widgets.length)
  assert.equal(new Set(listed.map((w) => w.id)).size, widgets.length)
  const known = new Set(AREAS.map((a) => a.id))
  for (const w of widgets) assert.ok(known.has(areaOf(w)), `${w.id}: unknown area ${areaOf(w)}`)
})

test('plugins go by their tags', () => {
  const plugin = (tags: string[]) => ({ ...widgets[0], kind: 'plugin' as const, bundle: undefined, tags })
  assert.equal(areaOf(plugin(['git', 'dev', 'benchmark'])), 'benchmark')
  assert.equal(areaOf(plugin(['example', 'dev'])), 'examples')
  assert.equal(areaOf(plugin(['ai', 'agents', 'usage'])), 'agents')
  assert.equal(areaOf(plugin(['tasks', 'todoist'])), 'personal')
  assert.equal(areaOf(plugin(['errors', 'monitoring', 'ops'])), 'ops')
})
