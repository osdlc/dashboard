// Run with `pnpm test` (node --test). Node strips the types itself.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { arcPath, ariaLabel, layout, percent, sliceColor, type Pie } from '../src/pie.ts'

const GIB = 1024 ** 3
const disk: Pie = {
  type: 'pie',
  label: 'Disk',
  format: 'bytes',
  slices: [
    { label: 'photos', value: 60 * GIB },
    { label: 'apps', value: 30 * GIB },
    { label: 'free', value: 10 * GIB, status: 'warn' },
  ],
}

test('slices get shares, formatted values and percents', () => {
  const s = layout(disk)
  assert.deepEqual(
    s.map((x) => [x.label, x.value, x.percent]),
    [
      ['photos', '60.0 GiB', '60%'],
      ['apps', '30.0 GiB', '30%'],
      ['free', '10.0 GiB', '10%'],
    ],
  )
  assert.equal(s[0]!.color, 'var(--series-1)')
  assert.equal(s[2]!.color, 'var(--warn)', 'a status wins')
  assert.ok(s.every((x) => x.path.startsWith('M ')))
})

test('the label sums up the slices for screen readers', () => {
  assert.equal(ariaLabel(disk, layout(disk)), 'Disk: photos 60.0 GiB, 60%; apps 30.0 GiB, 30%; free 10.0 GiB, 10%')
  const none: Pie = { type: 'pie', slices: [{ label: 'a', value: 0 }] }
  assert.deepEqual(layout(none), [])
  assert.equal(ariaLabel(none, []), 'Pie chart: no data')
  assert.deepEqual(layout({ type: 'pie', slices: [] }), [])
})

test('bad values count as zero; a zero slice draws nothing', () => {
  const s = layout({ type: 'pie', slices: [
    { label: 'a', value: 1 },
    { label: 'b', value: -5 },
    { label: 'c', value: Number.NaN },
    { label: 'd', value: 0 },
  ] })
  assert.deepEqual(s.map((x) => x.percent), ['100%', '0%', '0%', '0%'])
  assert.deepEqual(s.slice(1).map((x) => x.path), ['', '', ''])
})

test('arcs: a quarter wedge, a ring segment, and a whole ring in two halves', () => {
  assert.equal(arcPath(0, 0.25, 0), 'M 50.000 0.000 A 50 50 0 0 1 100.000 50.000 L 50 50 Z')
  const ring = arcPath(0, 0.75, 25)
  assert.match(ring, /^M 50\.000 0\.000 A 50 50 0 1 1 0\.000 50\.000 L 25\.000 50\.000 A 25 25 0 1 0 50\.000 25\.000 Z$/)
  const whole = arcPath(0, 1, 25)
  assert.equal(whole.match(/A /g)!.length, 4)
  assert.equal(arcPath(0.3, 0.3, 0), '')
})

test('colors: accent cycles the theme set, other scales spread end to end', () => {
  const s = [{ label: 'a', value: 1 }, { label: 'b', value: 1 }, { label: 'c', value: 1, status: 'unknown' as const }]
  assert.equal(sliceColor(undefined, 1, s), 'var(--series-2)')
  assert.equal(sliceColor('accent', 6, [...s, ...s, ...s]), 'var(--series-1)')
  assert.equal(sliceColor('accent', 0, [{ label: 'a', value: 1, tone: 'info' }]), 'var(--info)', 'a tone comes next')
  assert.equal(sliceColor('cool', 2, s), 'var(--muted)')
  assert.match(sliceColor('cool', 0, s), /var\(--cool-low\), var\(--cool-mid\) 0%/)
  assert.equal(percent(0.004), '<1%')
})
