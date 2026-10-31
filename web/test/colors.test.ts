// Run with `pnpm test` (node --test). Node strips the types itself.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import type { ColorTheme } from '../src/contract.ts'
import { builtinTheme, DEFAULT_THEME, seriesColor, themeCss, toneColor } from '../src/colors.ts'
import { BUILTIN_THEMES } from '../src/themes.ts'

/** The CSS variables a block sets, by name. */
function block(css: string, selector: string): Record<string, string> {
  const start = css.indexOf(`${selector} {`)
  assert.ok(start >= 0, `no ${selector} block`)
  const body = css.slice(start, css.indexOf('}', start))
  return Object.fromEntries([...body.matchAll(/--([a-z0-9-]+): (#[0-9a-f]{6});/g)].map((m) => [m[1], m[2]]))
}

test('every built-in theme sets every token in both variants', () => {
  assert.deepEqual(
    BUILTIN_THEMES.map((t) => t.name),
    ['default', 'contrast', 'paper', 'ember'],
  )
  for (const theme of BUILTIN_THEMES) {
    const css = themeCss(theme)
    assert.deepEqual(block(css, ':root'), theme.light, theme.name)
    assert.deepEqual(block(css, ":root:not([data-theme='light'])"), theme.dark, theme.name)
    assert.deepEqual(block(css, ":root[data-theme='dark']"), theme.dark, theme.name)
  }
})

test('the default theme keeps the original colors', () => {
  assert.equal(DEFAULT_THEME.name, 'default')
  assert.equal(DEFAULT_THEME.light.accent, '#2f6fde')
  assert.equal(DEFAULT_THEME.dark.bg, '#111317')
  assert.equal(builtinTheme('paper')?.name, 'paper')
  assert.equal(builtinTheme('nope'), undefined)
  assert.equal(builtinTheme(null), undefined)
})

test('values that are not hex colors never reach the CSS', () => {
  const bad: ColorTheme = {
    ...DEFAULT_THEME,
    light: { ...DEFAULT_THEME.light, accent: 'red; } body { display: none' },
  }
  const css = themeCss(bad)
  assert.ok(!css.includes('display'))
  assert.equal(block(css, ':root').accent, undefined)
})

test('tones and series colors are token variables', () => {
  assert.equal(toneColor('warn'), 'var(--warn)')
  assert.equal(seriesColor(0), 'var(--series-1)')
  assert.equal(seriesColor(7), 'var(--series-2)')
  assert.equal(seriesColor(2, 'error'), 'var(--error)')
})

test('style.css has no colors of its own', () => {
  const css = readFileSync(new URL('../src/style.css', import.meta.url), 'utf8')
  assert.deepEqual(css.match(/#[0-9a-f]{3,6}\b/gi) ?? [], [])
  for (const [, name] of css.matchAll(/var\(--([a-z0-9-]+)/g)) {
    const local = ['fill', 'line', 'tone', 'col']
    assert.ok(name! in DEFAULT_THEME.light || local.includes(name!), `style.css uses --${name}, which is not a token`)
  }
})
