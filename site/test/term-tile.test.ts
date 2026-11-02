// Run with `pnpm test` (node --test). Node strips the types itself.
import assert from 'node:assert/strict'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { cells, cssColor, styleCss, tileHtml, type TileCapture } from '../src/lib/term-tile.ts'

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..')

test('colors: terminal default, names, hex and 256-color indexes', () => {
  assert.equal(cssColor(''), '')
  assert.equal(cssColor('darkgray'), 'var(--tt-darkgray)')
  assert.equal(cssColor('#1e5ac8'), '#1e5ac8')
  assert.equal(cssColor('i2'), 'var(--tt-green)')
  assert.equal(cssColor('i196'), 'rgb(255,0,0)')
  assert.equal(cssColor('i232'), 'rgb(8,8,8)')
})

test('styles: modifiers, and reversed swaps the colors', () => {
  assert.equal(styleCss(['', '', '']), '')
  assert.equal(styleCss(['green', '', 'b']), 'color:var(--tt-green);font-weight:700')
  assert.equal(styleCss(['', '', 'r']), 'color:var(--tt-bg);background:var(--tt-fg)')
  assert.equal(styleCss(['red', '', 'ux']), 'color:var(--tt-red);text-decoration:underline line-through')
})

test('html: escapes text and pins non-ASCII runs to their cell width', () => {
  const html = tileHtml(
    { cols: 8, rows: 2, lines: [[[0, '┌ a<b ┐']], [[1, '██'], [0, ' ok']]] },
    [
      ['', '', ''],
      ['green', '', ''],
    ],
  )
  assert.equal(
    html,
    '<span class="w" style="width:1ch">┌</span> a&lt;b <span class="w" style="width:1ch">┐</span>\n' +
      '<span style="color:var(--tt-green)"><span class="w" style="width:2ch">██</span></span> ok',
  )
  assert.equal(cells('a😀'), 3)
  assert.equal(cells('[🛠️]'), 4)
})

test('every capture line fits its size', () => {
  const root = join(site, 'src/data/tui-tiles')
  if (!existsSync(root)) return
  for (const theme of readdirSync(root)) {
    for (const file of readdirSync(join(root, theme))) {
      const capture: TileCapture = JSON.parse(readFileSync(join(root, theme, file), 'utf8'))
      for (const [size, c] of Object.entries(capture.sizes)) {
        assert.equal(c.lines.length, c.rows, `${theme}/${file} ${size}`)
        for (const line of c.lines) {
          const width = line.reduce((n, [, text]) => n + cells(text), 0)
          assert.ok(width <= c.cols, `${theme}/${file} ${size}: a line is ${width} cells, wider than ${c.cols}`)
        }
      }
    }
  }
})
