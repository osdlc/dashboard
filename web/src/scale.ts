import type { Widget } from './contract'

export type ColorScale = NonNullable<Extract<Widget, { type: 'series' }>['color']>

/** Color for `t` in 0..1 on a color scale, mixed from the theme's three stops (CSS vars). */
export function scaleColor(scale: ColorScale | undefined, t: number): string {
  if (!scale || scale === 'accent') return 'var(--accent)'
  const c = Math.min(1, Math.max(0, t))
  const v = (stop: string) => `var(--${scale}-${stop})`
  return c < 0.5
    ? `color-mix(in oklab, ${v('low')}, ${v('mid')} ${(c * 200).toFixed(0)}%)`
    : `color-mix(in oklab, ${v('mid')}, ${v('high')} ${((c - 0.5) * 200).toFixed(0)}%)`
}
