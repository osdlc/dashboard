/**
 * Geometry, colors and text for the pie widget, kept apart from PieChart.vue
 * so `pnpm test` can run it in Node. Matches the TUI (crates/tui/src/pie.rs):
 * slices clockwise from twelve o'clock, the same colors and percent text.
 */
import type { Widget } from './contract'
import { formatValue } from './format.ts'
import { scaleColor } from './scale.ts'
import { seriesColor, toneColor } from './colors.ts'

export type Pie = Extract<Widget, { type: 'pie' }>
type Slice = Pie['slices'][number]

/** The viewBox is 0 0 100 100; the disc sits in the middle. */
const R = 50
/** Inner radius of the donut hole, as a share of the outer radius. */
export const HOLE = 0.55

export interface DrawnSlice {
  label: string
  value: string
  percent: string
  share: number
  color: string
  /** SVG path in the 100x100 viewBox; empty for a zero slice. */
  path: string
}

/** Negative and non-finite values count as zero. */
const clean = (v: number) => (Number.isFinite(v) ? Math.max(0, v) : 0)

export function percent(share: number): string {
  const pc = share * 100
  return pc > 0 && pc < 1 ? '<1%' : `${pc.toFixed(0)}%`
}

/** A status wins, then the slice's tone; `accent` cycles series-1..6; other scales spread the slices end to end. */
export function sliceColor(scale: Pie['color'], i: number, slices: readonly Slice[]): string {
  const status = slices[i]?.status
  if (status) return status === 'unknown' ? 'var(--muted)' : `var(--${status})`
  const tone = slices[i]?.tone
  if (tone) return toneColor(tone)
  if (!scale || scale === 'accent') return seriesColor(i)
  return scaleColor(scale, slices.length > 1 ? i / (slices.length - 1) : 0.5)
}

function point(turn: number, r: number): string {
  const a = turn * 2 * Math.PI
  return `${(R + r * Math.sin(a)).toFixed(3)} ${(R - r * Math.cos(a)).toFixed(3)}`
}

/**
 * The outline of the part of the ring from `from` to `to` (fractions of a
 * turn, clockwise from the top). `inner` 0 makes a wedge. A whole turn is
 * drawn as two halves, since one arc can't end where it starts.
 */
export function arcPath(from: number, to: number, inner: number): string {
  const span = to - from
  if (span <= 0) return ''
  if (span >= 1 - 1e-9) {
    const outer = `M ${point(0, R)} A ${R} ${R} 0 1 1 ${point(0.5, R)} A ${R} ${R} 0 1 1 ${point(0, R)} Z`
    if (inner <= 0) return outer
    const r = inner
    return `${outer} M ${point(0, r)} A ${r} ${r} 0 1 0 ${point(0.5, r)} A ${r} ${r} 0 1 0 ${point(0, r)} Z`
  }
  const large = span > 0.5 ? 1 : 0
  const outer = `M ${point(from, R)} A ${R} ${R} 0 ${large} 1 ${point(to, R)}`
  if (inner <= 0) return `${outer} L ${R} ${R} Z`
  return `${outer} L ${point(to, inner)} A ${inner} ${inner} 0 ${large} 0 ${point(from, inner)} Z`
}

/** Everything PieChart.vue draws. An empty list means nothing to show. */
export function layout(w: Pie): DrawnSlice[] {
  const total = w.slices.reduce((sum, s) => sum + clean(s.value), 0)
  if (!(total > 0)) return []
  const inner = w.donut === false ? 0 : R * HOLE
  let at = 0
  return w.slices.map((s, i) => {
    const share = clean(s.value) / total
    const path = arcPath(at, at + share, inner)
    at += share
    return {
      label: s.label,
      value: formatValue(s.value, w.format, w.unit),
      percent: percent(share),
      share,
      color: sliceColor(w.color, i, w.slices),
      path,
    }
  })
}

/** For screen readers and the hover title: "Disk: photos 60.0 GiB, 60%; apps 30.0 GiB, 30%". */
export function ariaLabel(w: Pie, slices: readonly DrawnSlice[]): string {
  const name = w.label ? `${w.label}: ` : 'Pie chart: '
  if (slices.length === 0) return `${name}no data`
  return name + slices.map((s) => `${s.label} ${s.value}, ${s.percent}`).join('; ')
}
