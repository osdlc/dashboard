// Turns a terminal tile capture (src/data/tui-tiles/<theme>/<id>.json, written
// by crates/tui/src/tile_shots.rs) into colored HTML for a <pre>.
//
// Each line is a list of [style index, text] runs. A style is [fg, bg,
// modifiers]: a color is '' (the terminal's own), a name ('green',
// 'darkgray'), '#rrggbb' or 'i<n>' (256-color index); modifiers are letters
// (b bold, d dim, i italic, u underlined, r reversed, x crossed out).
// Named colors become CSS variables (--tt-green), so the frame picks the
// terminal's palette.
//
// Runs of non-ASCII characters get a fixed width in `ch`, so lines stay
// aligned even when the browser draws box, block or braille characters with a
// fallback font of a slightly different width.

export type Style = [fg: string, bg: string, mods: string]
export type Run = [style: number, text: string]

export interface SizeCapture {
  cols: number
  rows: number
  lines: Run[][]
}

export interface TileCapture {
  width: number
  styles: Style[]
  sizes: Record<string, SizeCapture>
}

const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')

/** The 16 named colors, in index order. */
const NAMES = [
  'black',
  'red',
  'green',
  'yellow',
  'blue',
  'magenta',
  'cyan',
  'gray',
  'darkgray',
  'lightred',
  'lightgreen',
  'lightyellow',
  'lightblue',
  'lightmagenta',
  'lightcyan',
  'white',
]

/** A capture's color as CSS, or '' for the terminal's own. */
export function cssColor(c: string): string {
  if (!c) return ''
  if (c.startsWith('#')) return c
  const index = /^i(\d+)$/.exec(c)
  if (!index) return NAMES.includes(c) ? `var(--tt-${c})` : ''
  const n = Number(index[1])
  if (n < 16) return `var(--tt-${NAMES[n]})`
  if (n >= 232) {
    const v = 8 + (n - 232) * 10
    return `rgb(${v},${v},${v})`
  }
  const level = (k: number) => (k ? 55 + k * 40 : 0)
  const k = n - 16
  return `rgb(${level(Math.floor(k / 36))},${level(Math.floor(k / 6) % 6)},${level(k % 6)})`
}

/** Inline CSS for a style, or '' when it is the terminal's plain text. */
export function styleCss([fg, bg, mods]: Style): string {
  let color = cssColor(fg)
  let background = cssColor(bg)
  if (mods.includes('r')) {
    ;[color, background] = [background || 'var(--tt-bg)', color || 'var(--tt-fg)']
  }
  const css: string[] = []
  if (color) css.push(`color:${color}`)
  if (background) css.push(`background:${background}`)
  if (mods.includes('b')) css.push('font-weight:700')
  if (mods.includes('d')) css.push('opacity:.6')
  if (mods.includes('i')) css.push('font-style:italic')
  const lines = [mods.includes('u') && 'underline', mods.includes('x') && 'line-through'].filter(Boolean)
  if (lines.length) css.push(`text-decoration:${lines.join(' ')}`)
  return css.join(';')
}

/** Characters a terminal draws two cells wide. */
const WIDE = /[ᄀ-ᅟ⺀-꓏가-힣豈-﫿︰-﹏＀-｠￠-￦\u{1f300}-\u{1faff}]/u

const graphemes = new Intl.Segmenter('en', { granularity: 'grapheme' })

/** Cells a run of text takes: one per character, two for a wide one. */
export function cells(text: string): number {
  let n = 0
  for (const { segment } of graphemes.segment(text)) n += WIDE.test(segment) ? 2 : 1
  return n
}

/** Escaped text with each non-ASCII stretch pinned to its width in cells. */
function pinned(text: string): string {
  return text.replace(/[^\x00-\x7f]+/g, (s) => `<span class="w" style="width:${cells(s)}ch">${s}</span>`)
}

/** One size of a capture as HTML lines for a <pre>. */
export function tileHtml(capture: SizeCapture, styles: Style[]): string {
  const css = styles.map(styleCss)
  return capture.lines
    .map((runs) =>
      runs
        .map(([s, text]) => {
          const inner = pinned(esc(text))
          return css[s] ? `<span style="${css[s]}">${inner}</span>` : inner
        })
        .join(''),
    )
    .join('\n')
}
