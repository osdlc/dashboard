/**
 * The vertical range of the series widget, kept apart from SeriesChart.vue so
 * `pnpm test` can run it in Node. Same rule as `series_scale` in the Rust contract
 * crate (crates/contract/src/lib.rs), which the TUI uses.
 */

export interface Bounds {
  yMin?: number | null | undefined
  yMax?: number | null | undefined
  fit?: boolean | undefined
}

export interface Scale {
  lo: number
  hi: number
}

const EPS = 1e-9

/**
 * Bottom is `yMin`, else the smaller of 0 and the lowest value. Top is `yMax`, else
 * the highest value, at least 1 when the bottom is 0 or more and `yMin` is unset (so
 * small positive values draw as they always have). `fit` with no bounds is the data's
 * own range plus 5% padding. The top is always above the bottom.
 */
export function seriesScale(values: readonly number[], { yMin, yMax, fit }: Bounds = {}): Scale {
  const finite = values.filter(Number.isFinite)
  const has = finite.length > 0
  const lo = has ? Math.min(...finite) : 0
  const hi = has ? Math.max(...finite) : 0
  if (fit && yMin == null && yMax == null && has) {
    const pad = hi > lo ? (hi - lo) * 0.05 : Math.max(Math.abs(lo), 1) * 0.01
    return { lo: lo - pad, hi: hi + pad }
  }
  const bottom = yMin ?? Math.min(lo, 0)
  const top = yMax ?? (yMin == null && bottom >= 0 ? Math.max(hi, 1) : hi)
  return { lo: bottom, hi: Math.max(top, bottom + EPS) }
}

/** Where a value sits from the bottom of the scale, 0 to 1. */
export function ratio(s: Scale, v: number): number {
  return Math.min(1, Math.max(0, (v - s.lo) / (s.hi - s.lo)))
}

/** Where bars and fills start: zero, or the nearer edge when zero is off the scale. */
export function baseline(s: Scale): number {
  return Math.min(s.hi, Math.max(s.lo, 0))
}

/** Whether the scale has values on both sides of zero, so a zero line is drawn. */
export function spansZero(s: Scale): boolean {
  return s.lo < 0 && s.hi > 0
}
