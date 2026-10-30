import type { Widget } from './contract'

type ValueFormat = Extract<Widget, { type: 'series' }>['format']

/** "30s", "2m", "250ms": matches the TUI. */
export function formatMs(ms: number): string {
  if (ms % 60_000 === 0) return `${ms / 60_000}m`
  if (ms % 1000 === 0) return `${ms / 1000}s`
  return `${ms}ms`
}

/** Same output as `format_value` in the Rust contract crate. */
export function formatValue(value: number, format: ValueFormat | undefined, unit?: string | null): string {
  const u = unit ?? ''
  if (format === 'count') {
    const steps = ['', 'k', 'M', 'B']
    let v = Math.abs(value)
    let i = 0
    while (v >= 1000 && i < steps.length - 1) {
      v /= 1000
      i++
    }
    const sign = value < 0 ? '-' : ''
    return `${sign}${i === 0 || v >= 100 ? v.toFixed(0) : v.toFixed(1)}${steps[i]}${u}`
  }
  if (format !== 'bytes') return `${value.toFixed(0)}${u}`
  const steps = ['B', 'KiB', 'MiB', 'GiB', 'TiB']
  let v = Math.abs(value)
  let i = 0
  while (v >= 1024 && i < steps.length - 1) {
    v /= 1024
    i++
  }
  const sign = value < 0 ? '-' : ''
  return `${sign}${i === 0 ? v.toFixed(0) : v.toFixed(1)} ${steps[i]}${u}`
}

export interface Inline {
  text: string
  kind: 'plain' | 'strong' | 'code'
}

/** The small markdown subset text widgets support: `**bold**` and `` `code` ``. */
export function inlineMarkdown(text: string): Inline[] {
  return text
    .split(/(\*\*[^*]+\*\*|`[^`]+`)/)
    .filter(Boolean)
    .map((part) =>
      part.startsWith('**') && part.endsWith('**') && part.length > 4
        ? { text: part.slice(2, -2), kind: 'strong' }
        : part.startsWith('`') && part.endsWith('`') && part.length > 2
          ? { text: part.slice(1, -1), kind: 'code' }
          : { text: part, kind: 'plain' },
    )
}
