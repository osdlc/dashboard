<script setup lang="ts">
import { computed, ref, useId, watch } from 'vue'
import type { Widget } from '../contract'
import { formatValue } from '../format'
import { reducedMotion } from '../motion'
import { scaleColor } from '../scale'
import { seriesColor, toneColor } from '../colors'
import { baseline, ratio, seriesScale, spansZero } from '../seriesScale'

type Series = Extract<Widget, { type: 'series' }>
const props = defineProps<{ widget: Series }>()

const W = 600
const H = 100
const gid = useId()

const mirrored = computed(() => props.widget.mirrored === true && props.widget.series.length >= 2)
// Mirrored charts draw magnitudes: negatives count as 0.
const data = computed(() =>
  props.widget.series.map((s) =>
    s.points.map(([t, v]) => [Number(t), mirrored.value ? Math.max(0, Number(v)) : Number(v)] as const),
  ),
)

const range = computed(() => {
  const flat = data.value.flat()
  const t0 = Math.min(...flat.map((p) => p[0]))
  const t1 = Math.max(...flat.map((p) => p[0]))
  const values = flat.map((p) => p[1])
  const scale = mirrored.value
    ? { lo: 0, hi: props.widget.y_max ?? Math.max(1, ...values) }
    : seriesScale(values, { yMin: props.widget.y_min, yMax: props.widget.y_max, fit: props.widget.fit })
  return { t0, t1, scale, ok: flat.length >= 2 }
})

/** SVG y of a value on the full-height scale (top is 0). */
const yOf = (v: number) => H - ratio(range.value.scale, v) * H
/** Where fills and bars start, and the zero line when the scale spans zero. */
const baseY = computed(() => yOf(baseline(range.value.scale)))
const zeroY = computed(() => (!mirrored.value && range.value.ok && spansZero(range.value.scale) ? yOf(0) : null))

/** Path for one series; `y` maps a value to its SVG y and `base` is where an area closes. */
function path(pts: ReadonlyArray<readonly [number, number]>, base: number, y: (v: number) => number, area: boolean) {
  const { t0, t1 } = range.value
  const x = (t: number) => ((t - t0) / Math.max(1, t1 - t0)) * W
  const line = pts.map(([t, v], i) => `${i ? 'L' : 'M'}${x(t).toFixed(1)},${y(v).toFixed(1)}`).join('')
  const first = pts[0]
  const last = pts.at(-1)
  if (!area || !first || !last) return line
  return `${line}L${x(last[0]).toFixed(1)},${base}L${x(first[0]).toFixed(1)},${base}Z`
}

const paths = computed(() => {
  if (!range.value.ok || bars.value.length) return []
  const area = props.widget.style === 'area' || mirrored.value
  const tone = (i: number) => props.widget.series[i]?.tone
  // Area fills use the color scale (or accent) unless the series names a tone;
  // lines each get the next categorical color.
  const look = (i: number) => {
    const t = tone(i)
    return { '--fill': t ? toneColor(t) : fill.value, '--line': seriesColor(i, t) }
  }
  if (mirrored.value) {
    const [down = [], up = []] = data.value
    const top = range.value.scale.hi
    const half = (dir: number) => (v: number) => H / 2 + dir * (Math.min(v, top) / top) * (H / 2)
    return [
      { d: path(down, H / 2, half(-1), area), cls: 'a', style: look(0) },
      { d: path(up, H / 2, half(1), area), cls: 'b', style: look(1) },
    ]
  }
  return data.value.map((pts, i) => ({ d: path(pts, baseY.value, yOf, area), cls: `${i === 0 ? 'a' : 'b'} s${i}`, style: look(i) }))
})

/** Latest value of each series. Several lines also name themselves, in their own color. */
const latest = computed(() => {
  const w = props.widget
  const arrows = mirrored.value ? ['↓', '↑'] : ['', '']
  const named = !mirrored.value && w.series.length > 1
  return w.series
    .slice(0, mirrored.value ? 2 : 6)
    .map((s, i) => {
      const last = s.points.at(-1)
      if (!last) return null
      const value = formatValue(Number(last[1]), w.format, w.unit)
      return {
        text: named ? `${s.name} ${value}` : `${arrows[i]}${value}`,
        color: named ? seriesColor(i, s.tone) : undefined,
      }
    })
    .filter((x) => x !== null)
})

/** Gradient stops' CSS var prefix, or null for the flat accent color. */
const colorScale = computed(() => (props.widget.color && props.widget.color !== 'accent' ? props.widget.color : null))
const fill = computed(() => (colorScale.value ? `url(#${gid})` : 'var(--accent)'))

/** Bars style: one rect per sample of the first series, each colored by its own height. */
const bars = computed(() => {
  const pts = data.value[0] ?? []
  if (props.widget.style !== 'bars' || mirrored.value || pts.length === 0) return []
  const w = W / pts.length
  const tone = props.widget.series[0]?.tone
  const base = baseY.value
  return pts.map(([, v], i) => {
    const t = ratio(range.value.scale, v)
    const color = tone ? toneColor(tone) : scaleColor(props.widget.color, t)
    const y = yOf(v)
    return { x: i * w + w * 0.1, w: w * 0.8, y: Math.min(y, base), h: Math.abs(base - y), color }
  })
})

// When the window moves (history is full), draw the new frame shifted right by the
// distance the data moved, then slide it back to 0 so the chart scrolls instead of jumping.
const shift = ref(0)
const slideMs = ref(0)
let lastAt = 0
watch(
  () => [range.value.t0, range.value.t1] as const,
  ([t0, t1], [pt0, pt1]) => {
    const now = performance.now()
    const elapsed = now - lastAt
    lastAt = now
    if (reducedMotion() || !(t0 > pt0 && t1 > pt1)) return
    const pts = data.value[0] ?? []
    const x = bars.value.length
      ? (pts.filter(([t]) => t > pt1).length * W) / pts.length
      : (W * (t1 - pt1)) / Math.max(1, t1 - t0)
    slideMs.value = 0
    shift.value = x
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        slideMs.value = Math.min(elapsed, 2000)
        shift.value = 0
      }),
    )
  },
  { flush: 'pre' },
)
const slide = computed(() => ({
  transform: `translateX(${shift.value}px)`,
  transition: slideMs.value ? `transform ${slideMs.value}ms linear` : 'none',
}))
</script>

<template>
  <figure class="w-series" :data-style="widget.style ?? 'line'" :data-mirrored="mirrored || undefined">
    <figcaption>
      {{ widget.label }}
      <strong v-if="latest.length">
        <span v-for="(l, i) in latest" :key="i" :style="l.color ? { color: l.color } : undefined">{{ i ? ' ' : '' }}{{ l.text }}</span>
      </strong>
    </figcaption>
    <svg :viewBox="`0 0 ${W} ${H}`" preserveAspectRatio="none" role="img" :aria-label="widget.label">
      <defs v-if="colorScale">
        <!-- Colored by height, like the TUI: for heat, low is green and high is red. -->
        <linearGradient :id="gid" x1="0" y1="1" x2="0" y2="0">
          <stop offset="0%" :stop-color="`var(--${colorScale}-low)`" />
          <stop offset="50%" :stop-color="`var(--${colorScale}-mid)`" />
          <stop offset="100%" :stop-color="`var(--${colorScale}-high)`" />
        </linearGradient>
      </defs>
      <line v-if="mirrored" class="axis" x1="0" :y1="H / 2" :x2="W" :y2="H / 2" vector-effect="non-scaling-stroke" />
      <line v-if="zeroY !== null" class="axis zero" x1="0" :y1="zeroY" :x2="W" :y2="zeroY" vector-effect="non-scaling-stroke" />
      <g :style="slide">
      <rect v-for="(b, i) in bars" :key="`b${i}`" :x="b.x" :y="b.y" :width="b.w" :height="b.h" :fill="b.color" rx="1" />
      <path
        v-for="(p, i) in paths"
        :key="i"
        :d="p.d"
        :class="p.cls"
        :style="p.style"
        vector-effect="non-scaling-stroke"
      />
      </g>
    </svg>
  </figure>
</template>
