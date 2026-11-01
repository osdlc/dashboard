<script setup lang="ts">
import { computed, ref, useId, watch } from 'vue'
import type { Widget } from '../contract'
import { formatValue } from '../format'
import { reducedMotion } from '../motion'
import { scaleColor } from '../scale'
import { seriesColor, toneColor } from '../colors'

type Series = Extract<Widget, { type: 'series' }>
const props = defineProps<{ widget: Series }>()

const W = 600
const H = 100
const gid = useId()

const data = computed(() => props.widget.series.map((s) => s.points.map(([t, v]) => [Number(t), Math.max(0, Number(v))] as const)))
const mirrored = computed(() => props.widget.mirrored === true && data.value.length >= 2)

const range = computed(() => {
  const flat = data.value.flat()
  const t0 = Math.min(...flat.map((p) => p[0]))
  const t1 = Math.max(...flat.map((p) => p[0]))
  const yMax = props.widget.y_max ?? Math.max(1, ...flat.map((p) => p[1]))
  return { t0, t1, yMax, ok: flat.length >= 2 }
})

/** Path for one series. `up` grows from `base` upward, otherwise downward. */
function path(pts: ReadonlyArray<readonly [number, number]>, base: number, height: number, up: boolean, area: boolean) {
  const { t0, t1, yMax } = range.value
  const x = (t: number) => ((t - t0) / Math.max(1, t1 - t0)) * W
  const y = (v: number) => base + (up ? -1 : 1) * (Math.min(v, yMax) / yMax) * height
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
    return [
      { d: path(down, H / 2, H / 2, true, area), cls: 'a', style: look(0) },
      { d: path(up, H / 2, H / 2, false, area), cls: 'b', style: look(1) },
    ]
  }
  return data.value.map((pts, i) => ({ d: path(pts, H, H, true, area), cls: `${i === 0 ? 'a' : 'b'} s${i}`, style: look(i) }))
})

const latest = computed(() => {
  const arrows = mirrored.value ? ['↓', '↑'] : ['', '']
  return props.widget.series
    .slice(0, 2)
    .map((s, i) => {
      const last = s.points.at(-1)
      return last ? `${arrows[i]}${formatValue(Number(last[1]), props.widget.format, props.widget.unit)}` : ''
    })
    .filter(Boolean)
    .join(' ')
})

/** Gradient stops' CSS var prefix, or null for the flat accent color. */
const colorScale = computed(() => (props.widget.color && props.widget.color !== 'accent' ? props.widget.color : null))
const fill = computed(() => (colorScale.value ? `url(#${gid})` : 'var(--accent)'))

/** Bars style: one rect per sample of the first series, each colored by its own height. */
const bars = computed(() => {
  const pts = data.value[0] ?? []
  if (props.widget.style !== 'bars' || mirrored.value || pts.length === 0) return []
  const { yMax } = range.value
  const w = W / pts.length
  const tone = props.widget.series[0]?.tone
  return pts.map(([, v], i) => {
    const t = Math.min(v, yMax) / yMax
    const color = tone ? toneColor(tone) : scaleColor(props.widget.color, t)
    return { x: i * w + w * 0.1, w: w * 0.8, y: H - t * H, h: t * H, color }
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
      <strong v-if="latest">{{ latest }}</strong>
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
