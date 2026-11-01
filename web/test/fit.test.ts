// Run with `pnpm test` (node --test). Node strips the types itself.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { dropOrder, fitCount, tableColumns } from '../src/fit.ts'

test('items fit up to the first one that ends below the limit', () => {
  assert.equal(fitCount([20, 40, 60, 80], 65), 3)
  assert.equal(fitCount([20, 40], 65), 2)
  assert.equal(fitCount([70, 90], 65), 0)
  assert.equal(fitCount([], 65), 0)
  // Rounding in layout: half a pixel over still fits.
  assert.equal(fitCount([65.4], 65), 1)
})

test('a too-wide table drops low priorities first, the rightmost of equal ones, never the first', () => {
  // pod 3, namespace -1, status 2, ready 1, restarts and age unset.
  assert.deepEqual(dropOrder([3, -1, 2, 1, null, undefined]), [1, 5, 4, 3, 2])
  assert.deepEqual(dropOrder([null, null, null]), [2, 1])
  // The first column is kept even with the lowest priority.
  assert.deepEqual(dropOrder([-5, 0]), [1])
  assert.deepEqual(dropOrder([]), [])
})

test('a narrow table keeps the columns whose least widths fit, then narrows the widest', () => {
  const natural = [200, 80, 150, 50]
  const least = [100, 50, 50, 50]
  // Everything fits: nothing changes.
  assert.deepEqual(tableColumns(natural, least, [], 500), [[0, 200], [1, 80], [2, 150], [3, 50]])
  // Room for all four least widths: the widest give up width.
  assert.deepEqual(tableColumns(natural, least, [], 400), [[0, 135], [1, 80], [2, 135], [3, 50]])
  // Not enough: the rightmost column of equal priority goes first.
  assert.deepEqual(
    tableColumns(natural, least, [], 220).map(([i]) => i),
    [0, 1, 2],
  )
  // Priority beats position.
  assert.deepEqual(
    tableColumns(natural, least, [3, -1, 2, 1], 220).map(([i]) => i),
    [0, 2, 3],
  )
  // The first column alone, cut to the room.
  assert.deepEqual(tableColumns(natural, least, [], 60), [[0, 60]])
})
