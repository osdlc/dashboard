// Run with `pnpm test` (node --test). Node strips the types itself.
import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { COLOR_THEMES } from '../src/lib/color-themes.ts'
import { fields, inlineCode, typesIn, widgetTypes } from '../src/lib/fields.ts'

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repo = resolve(site, '..')
const schema = JSON.parse(readFileSync(join(site, 'src/data/view-schema.json'), 'utf8'))

test('the color theme picker lists the contract built-ins', () => {
  const rust = readFileSync(join(repo, 'crates/contract/src/theme.rs'), 'utf8')
  const line = /pub const BUILTIN: &\[&str\] = &\[([^\]]*)\]/.exec(rust)?.[1]
  assert.ok(line, 'BUILTIN not found in theme.rs')
  const names = line.split(',').map((s) => s.trim().replace(/^"|"$/g, '').replace(/^DEFAULT$/, 'default'))
  assert.deepEqual([...COLOR_THEMES], names)
})

test('every widget type has fields, required ones first', () => {
  const types = widgetTypes(schema)
  assert.ok(types.includes('pie') && types.includes('row'), types.join(' '))
  for (const t of types) {
    const f = fields(schema, t).filter((x) => !x.nested)
    assert.ok(f.length, t)
    const firstOptional = f.findIndex((x) => !x.required)
    if (firstOptional >= 0) assert.ok(f.slice(firstOptional).every((x) => !x.required), t)
  }
})

test('fields show list items, enums and defaults', () => {
  const gauge = fields(schema, 'gauge')
  assert.deepEqual(
    gauge.map((f) => f.key),
    ['label', 'percent', 'caption', 'marker', 'thresholds', 'thresholds.error', 'thresholds.warn'],
  )
  const list = fields(schema, 'list')
  assert.ok(list.some((f) => f.key === 'items[].title' && f.required && f.nested))
  const series = Object.fromEntries(fields(schema, 'series').map((f) => [f.key, f]))
  assert.equal(series.style.type, 'line | area | bars')
  assert.equal(series.style.default, '"line"')
  assert.equal(series['series[].points'].type, '[ms, value] list')
  assert.equal(fields(schema, 'pie').find((f) => f.key === 'donut')?.default, 'true')
  assert.equal(fields(schema, 'row')[0].type, 'widget list')
})

test('typesIn finds children and table cells', () => {
  const view = {
    type: 'row',
    children: [
      { type: 'stat', label: 'a', value: '1' },
      { type: 'table', columns: [], rows: [{ cells: { a: 'x', b: { type: 'gauge', percent: 1 } } }] },
    ],
  }
  assert.deepEqual(typesIn(view), ['row', 'stat', 'table', 'gauge'])
})

test('every block uses only known widget types', () => {
  const types = new Set(widgetTypes(schema))
  for (const f of readdirSync(join(repo, 'examples/blocks'))) {
    const b = JSON.parse(readFileSync(join(repo, 'examples/blocks', f), 'utf8'))
    for (const t of typesIn(b.view.root)) assert.ok(types.has(t), `${f}: ${t}`)
  }
})

test('inlineCode escapes and marks backticks', () => {
  assert.equal(inlineCode('a `<b>` c'), 'a <code>&lt;b&gt;</code> c')
})
