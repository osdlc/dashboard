// Run with `pnpm test` (node --test). Node strips the types itself.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { Widget } from '../src/contract.ts'
import { back, dirForKey, enter, itemIndex, items, moved, neighbor, NO_FOCUS, type Box, type Focus } from '../src/nav.ts'

const list = (titles: string[]): Widget => ({
  type: 'list',
  items: titles.map((t) => ({ title: t, action: { type: 'open_url', url: `https://example.com/${t}` } })),
})

/** a b / c d / e e, in pixels with 16px gaps. */
const grid: Array<[string, Box]> = [
  ['a', { x: 0, y: 0, width: 300, height: 100 }],
  ['b', { x: 316, y: 0, width: 300, height: 100 }],
  ['c', { x: 0, y: 116, width: 300, height: 100 }],
  ['d', { x: 316, y: 116, width: 300, height: 100 }],
  ['e', { x: 0, y: 232, width: 616, height: 100 }],
]
const tile = (id: string): Focus => ({ kind: 'tile', tile: id })

test('arrows move to the nearest tile that way', () => {
  assert.deepEqual(moved(NO_FOCUS, 'down', grid, []), tile('a'))
  assert.deepEqual(moved(tile('a'), 'right', grid, []), tile('b'))
  assert.deepEqual(moved(tile('a'), 'down', grid, []), tile('c'))
  assert.deepEqual(moved(tile('b'), 'down', grid, []), tile('d'))
  assert.deepEqual(moved(tile('d'), 'left', grid, []), tile('c'))
  assert.deepEqual(moved(tile('d'), 'down', grid, []), tile('e'))
  assert.deepEqual(moved(tile('e'), 'up', grid, []), tile('c'), 'ties go to the first')
  assert.deepEqual(moved(tile('a'), 'up', grid, []), tile('a'), 'nothing that way: stay')
  assert.deepEqual(moved(tile('gone'), 'up', grid, []), NO_FOCUS)
})

test('prefers tiles in line over closer ones off to the side', () => {
  const boxes: Box[] = [
    { x: 0, y: 0, width: 40, height: 20 },
    { x: 40, y: 0, width: 60, height: 10 },
    { x: 40, y: 10, width: 60, height: 10 },
    { x: 0, y: 20, width: 100, height: 5 },
  ]
  assert.equal(neighbor(boxes, 2, 'left'), 0)
  assert.equal(neighbor(boxes, 0, 'down'), 3, 'touching edges do not line up')
})

test('enter goes in, moves, runs the action; esc comes back', () => {
  const its = items(list(['one', 'two', 'three']))
  assert.deepEqual(enter(tile('t'), []), [tile('t'), null])
  let [f] = enter(tile('t'), its)
  assert.equal(itemIndex(f, its), 0)
  f = moved(f, 'up', [], its)
  assert.equal(itemIndex(f, its), 0, 'stops at the top')
  for (let i = 0; i < 3; i++) f = moved(f, 'down', [], its)
  assert.equal(itemIndex(f, its), 2, 'stops at the bottom')
  assert.deepEqual(enter(f, its)[1], { type: 'open_url', url: 'https://example.com/three' })
  assert.deepEqual(back(f), tile('t'))
  assert.deepEqual(back(tile('t')), NO_FOCUS)
})

test('focus follows the item across updates', () => {
  const before = items(list(['a', 'b', 'c', 'd']))
  let f = enter(tile('t'), before)[0]
  f = moved(moved(f, 'down', [], before), 'down', [], before)
  assert.equal(itemIndex(f, items(list(['new', 'a', 'b', 'c', 'd']))), 3, 'still on c')
  assert.equal(itemIndex(f, items(list(['a', 'b', 'd']))), 2, 'c gone: nearest')
  assert.equal(itemIndex(f, items(list(['a']))), 0)
  assert.equal(itemIndex(f, []), null)
})

test('items come from nested lists, tables, uptime and bars', () => {
  const w: Widget = {
    type: 'column',
    children: [
      list(['x', 'x']),
      { type: 'table', columns: [{ key: 'n', label: 'N' }], rows: [{ cells: { n: 'row1' } }] },
      { type: 'row', children: [{ type: 'uptime', checks: [{ name: 'api', status: 'ok', url: 'https://api' }] }] },
      { type: 'bars', bars: [{ label: 'cargo', value: 1 }] },
    ],
  }
  const its = items(w)
  assert.deepEqual(
    its.map((i) => i.key),
    ['x', 'x#2', 'row1', 'api', 'cargo'],
  )
  assert.deepEqual(its[3]!.action, { type: 'open_url', url: 'https://api' })
})

test('keys map to directions', () => {
  assert.equal(dirForKey('ArrowLeft'), 'left')
  assert.equal(dirForKey('j'), 'down')
  assert.equal(dirForKey('x'), null)
})
