import type { ColorTheme, Theme, Tokens, Tone } from './contract'
import { BUILTIN_THEMES } from './themes.ts'

/**
 * Color themes. The tokens come from the Rust contract (the host sends the theme in
 * use with the settings; themes.ts has the built-ins) and become CSS variables named
 * after them: `--accent`, `--series-1`, `--heat-low`, ... style.css uses only those.
 */

export const DEFAULT_THEME: ColorTheme = BUILTIN_THEMES[0]!

/** A built-in theme by name. */
export function builtinTheme(name: string | null | undefined): ColorTheme | undefined {
  return BUILTIN_THEMES.find((t) => t.name === name)
}

const HEX = /^#[0-9a-f]{6}$/i

function vars(tokens: Tokens, indent: string): string {
  return Object.entries(tokens)
    .filter(([name, value]) => /^[a-z0-9-]+$/.test(name) && HEX.test(value))
    .map(([name, value]) => `${indent}--${name}: ${value};`)
    .join('\n')
}

/**
 * CSS for both variants: light by default; dark when the OS asks for it (`theme: auto`
 * leaves `data-theme` off) or when `data-theme="dark"`.
 */
export function themeCss(theme: ColorTheme): string {
  return [
    `:root {\n${vars(theme.light, '  ')}\n}`,
    `@media (prefers-color-scheme: dark) {\n  :root:not([data-theme='light']) {\n${vars(theme.dark, '    ')}\n  }\n}`,
    `:root[data-theme='dark'] {\n${vars(theme.dark, '  ')}\n}`,
  ].join('\n')
}

/** Show `theme`'s colors, replacing the ones shown before. No reload needed. */
export function applyColorTheme(theme: ColorTheme) {
  let el = document.getElementById('color-theme')
  if (!el) {
    el = document.createElement('style')
    el.id = 'color-theme'
    document.head.prepend(el)
  }
  const css = themeCss(theme)
  if (el.textContent !== css) el.textContent = css
}

/** `auto` removes the attribute so the CSS follows prefers-color-scheme. */
export function applyMode(theme: Theme) {
  const root = document.documentElement
  if (theme === 'auto') delete root.dataset.theme
  else root.dataset.theme = theme
}

/** The CSS color for a widget's `tone`. */
export function toneColor(tone: Tone): string {
  return `var(--${tone})`
}

/** The color of series `i` when it names none: the categorical tokens in order. */
export function seriesColor(i: number, tone?: Tone | null): string {
  return toneColor(tone ?? (`series-${(i % 6) + 1}` as Tone))
}
