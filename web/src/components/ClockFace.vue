<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { Widget } from '../contract'
import { forward, hourAngle, minuteAngle, secondAngle, ticks, timeText } from '../clockface'

type Face = Extract<Widget, { type: 'clock_face' }>
const props = defineProps<{ widget: Face }>()

// Angles only grow, so a hand going from 59 to 0 turns on instead of
// spinning back across the dial.
const hour = ref(hourAngle(props.widget.hour, props.widget.minute))
const minute = ref(minuteAngle(props.widget.minute))
const second = ref(secondAngle(props.widget.second ?? 0))
watch(
  () => props.widget,
  (w) => {
    hour.value = forward(hour.value, hourAngle(w.hour, w.minute))
    minute.value = forward(minute.value, minuteAngle(w.minute))
    if (w.second != null) second.value = forward(second.value, secondAngle(w.second))
  },
)

const label = computed(() => {
  const w = props.widget
  return `${w.label ? w.label + ' ' : ''}${timeText(w.hour, w.minute, w.second)}`
})
const marks = ticks()
const turn = (deg: number) => ({ transform: `rotate(${deg}deg)` })
</script>

<template>
  <div class="w-clockface" :data-status="widget.status ?? undefined">
    <div v-if="widget.label" class="w-label">{{ widget.label }}</div>
    <svg class="cf-dial" viewBox="-50 -50 100 100" role="img" :aria-label="label">
      <title>{{ label }}</title>
      <circle class="cf-face" r="48" />
      <line
        v-for="t in marks"
        :key="t.angle"
        class="cf-tick"
        :class="{ quarter: t.quarter }"
        :style="turn(t.angle)"
        x1="0"
        :y1="t.quarter ? -38 : -41"
        x2="0"
        y2="-44"
      />
      <line class="cf-hand cf-hour" :style="turn(hour)" x1="0" y1="4" x2="0" y2="-25" />
      <line class="cf-hand cf-minute" :style="turn(minute)" x1="0" y1="5" x2="0" y2="-37" />
      <line v-if="widget.second != null" class="cf-hand cf-second" :style="turn(second)" x1="0" y1="9" x2="0" y2="-40" />
      <circle class="cf-pin" r="2.4" />
    </svg>
    <div v-if="widget.caption" class="cf-caption">{{ widget.caption }}</div>
  </div>
</template>
