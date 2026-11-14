// Run with `pnpm test` (node --test). Node strips the types itself.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { formatValue } from '../src/format.ts'
import { baseline, ratio, seriesScale, spansZero } from '../src/seriesScale.ts'

test('charts without negatives scale 0..max, at least 1, exactly as before', () => {
  assert.deepEqual(seriesScale([3, 40, 12]), { lo: 0, hi: 40 })
  assert.deepEqual(seriesScale([0.2, 0.5]), { lo: 0, hi: 1 })
  assert.deepEqual(seriesScale([]), { lo: 0, hi: 1 })
  assert.deepEqual(seriesScale([3], { yMax: 100 }), { lo: 0, hi: 100 })
})

test('negative values lower the bottom; the top is the largest value', () => {
  assert.deepEqual(seriesScale([-2, 1, 0.5]), { lo: -2, hi: 1 })
  assert.deepEqual(seriesScale([-3, -1]), { lo: -3, hi: -1 })
  assert.deepEqual(seriesScale([-2, 1], { yMin: -5, yMax: 5 }), { lo: -5, hi: 5 })
  // y_min alone drops the "at least 1" floor.
  assert.deepEqual(seriesScale([2, 4], { yMin: 1 }), { lo: 1, hi: 4 })
})

test('the top stays above the bottom', () => {
  const s = seriesScale([5, 5], { yMin: 5 })
  assert.ok(s.hi > s.lo)
})

test('fit scales to the data with padding, unless bounds are set', () => {
  const s = seriesScale([354, 357], { fit: true })
  assert.ok(Math.abs(s.lo - 353.85) < 1e-9 && Math.abs(s.hi - 357.15) < 1e-9)
  assert.deepEqual(seriesScale([354, 357], { fit: true, yMax: 400 }), { lo: 0, hi: 400 })
  const flat = seriesScale([100, 100], { fit: true })
  assert.ok(flat.lo < 100 && flat.hi > 100)
})

test('zero line and baseline', () => {
  const s = seriesScale([-2, 6])
  assert.ok(spansZero(s))
  assert.equal(baseline(s), 0)
  assert.equal(ratio(s, 0), 0.25)
  assert.ok(!spansZero(seriesScale([1, 2])))
  assert.equal(baseline({ lo: 353, hi: 357 }), 353)
  assert.equal(baseline({ lo: -3, hi: -1 }), -1)
})

test('small percentages keep one decimal and their sign', () => {
  assert.equal(formatValue(-1.24, 'plain', '%'), '-1.2%')
  assert.equal(formatValue(0.04, 'plain', '%'), '0.0%')
  assert.equal(formatValue(-0.04, 'plain', '%'), '0.0%')
  assert.equal(formatValue(5, 'plain', '%'), '5%')
  assert.equal(formatValue(12.6, 'plain', '%'), '13%')
  assert.equal(formatValue(-1.24, 'plain', ' ms'), '-1 ms')
})
