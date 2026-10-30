<script setup lang="ts">
import { onUnmounted, ref, watch } from 'vue'
import { reducedMotion } from '../motion'

/** Shows `value`; when it changes from one number to another, counts there. */
const props = defineProps<{ value: string }>()
const DURATION = 500

const shown = ref(props.value)
let frame = 0

/** The number in a string like "1,204.5", and how many decimals it shows. */
function parse(s: string): { n: number; decimals: number } | null {
  const m = /^-?[\d,]*\.?\d+$/.exec(s.trim())
  if (!m) return null
  const plain = s.replace(/,/g, '')
  return { n: Number(plain), decimals: plain.split('.')[1]?.length ?? 0 }
}

watch(
  () => props.value,
  (to, from) => {
    cancelAnimationFrame(frame)
    const a = parse(from)
    const b = parse(to)
    if (!a || !b || reducedMotion()) {
      shown.value = to
      return
    }
    const start = performance.now()
    const step = (now: number) => {
      const t = Math.min(1, (now - start) / DURATION)
      const eased = 1 - (1 - t) ** 3
      shown.value = (a.n + (b.n - a.n) * eased).toFixed(b.decimals)
      if (t < 1) frame = requestAnimationFrame(step)
      else shown.value = to
    }
    frame = requestAnimationFrame(step)
  },
)
onUnmounted(() => cancelAnimationFrame(frame))
</script>

<template>{{ shown }}</template>
