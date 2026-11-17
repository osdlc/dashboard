// Turns a plain-text render of the terminal UI (from scripts/tui-snapshots.sh)
// into colored HTML. The text has no colors, so this guesses them the way the
// TUI draws them: borders dim, tile titles bold, meters colored by value,
// braille charts in the accent color.
//
// Every run of non-ASCII characters gets a fixed width in `ch`, so lines stay
// aligned even when the browser draws box, block or braille characters with a
// fallback font of a slightly different width.

const BOX = /[─-╿]/
const BLOCK = /[▀-▟]/
const BRAILLE = /[⠀-⣿]/

type Kind = 'box' | 'block' | 'braille' | 'dot' | 'bullet' | 'marker' | 'arrow' | 'text'

function kindOf(ch: string): Kind {
  if (ch === '·') return 'dot'
  if (ch === '●') return 'bullet'
  if (ch === '┃') return 'marker'
  if (ch === '▲' || ch === '▼' || ch === '║') return 'arrow'
  if (BOX.test(ch)) return 'box'
  if (BLOCK.test(ch)) return 'block'
  if (BRAILLE.test(ch)) return 'braille'
  return 'text'
}

const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')

function heat(value: number): string {
  if (value >= 80) return 'hot'
  if (value >= 50) return 'warm'
  return 'cool'
}

function span(cls: string, text: string): string {
  const wide = /[^\x00-\x7f]/.test(text)
  const style = wide ? ` style="width:${[...text].length}ch"` : ''
  return `<span class="t-${cls}"${style}>${esc(text)}</span>`
}

function line(raw: string, index: number, current: string): string {
  const chars = [...raw.replace(/\s+$/, '')]
  // First line: the screen tabs. Highlight the current one.
  if (index === 0) {
    return raw
      .replace(/\s+$/, '')
      .split('│')
      .map((tab) => {
        const name = tab.trim()
        return name.endsWith(` ${current}`) ? tab.replace(name, span('tab-on', name)) : span('dim', tab)
      })
      .join(span('box', '│'))
  }
  // Help line at the bottom: keys bold.
  if (/^\s*\? help/.test(raw)) {
    return esc(raw.replace(/\s+$/, '')).replace(/(\?|\bs\b|Tab|\bq\b)(?= )/g, '<span class="t-key">$1</span>')
  }

  // Group into runs of the same kind.
  const runs: { kind: Kind; text: string }[] = []
  for (const ch of chars) {
    const kind = kindOf(ch)
    const last = runs[runs.length - 1]
    if (last && last.kind === kind) last.text += ch
    else runs.push({ kind, text: ch })
  }

  let out = ''
  for (let i = 0; i < runs.length; i++) {
    const { kind, text } = runs[i]
    if (kind === 'box') {
      out += span('box', text)
      // A tile title follows "┌ " in a top border.
      const next = runs[i + 1]
      if (text.endsWith('┌') && next?.kind === 'text') {
        const m = /^( )(\S[^]*?)( ?)$/.exec(next.text)
        if (m) {
          out += esc(m[1]) + span('title', m[2]) + esc(m[3])
          i++
        }
      }
      continue
    }
    if (kind === 'block') {
      // The scrollbar thumb at the right edge.
      if (i === runs.length - 1 && runs[i - 1]?.kind === 'box') {
        out += span('scroll', text)
        continue
      }
      // A meter or gauge: color it by the number after the bar, if any.
      const rest = runs.slice(i + 1).map((r) => r.text).join('')
      const m = /^[\u2580-\u259f·┃\s]*(\d+(?:\.\d+)?)/.exec(rest)
      const value = m ? Number(m[1]) : NaN
      const cls = Number.isNaN(value) || value > 100 ? 'bar' : heat(value)
      out += span(cls, text)
      continue
    }
    if (kind === 'braille') out += span('chart', text)
    else if (kind === 'dot') out += span('faint', text)
    else if (kind === 'bullet') out += span('ok', text)
    else if (kind === 'marker') out += span('marker', text)
    else if (kind === 'arrow') out += span('scroll', text)
    else out += esc(text)
  }
  return out
}

export function tuiHtml(text: string, current: string): string {
  return text
    .replace(/\n+$/, '')
    .split('\n')
    .map((l, i) => line(l, i, current))
    .join('\n')
}
