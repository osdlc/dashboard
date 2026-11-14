<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { Widget } from '../contract'
import { fitFontSize } from '../display'

type Display = Extract<Widget, { type: 'display' }>
const props = defineProps<{ widget: Display }>()

/** The area the text may fill, and a hidden copy of the text at PROBE px to measure. */
const box = ref<HTMLElement>()
const probe = ref<HTMLElement>()
const size = ref(24)
const PROBE = 100

function fit() {
  const b = box.value
  const t = probe.value
  if (!b || !t) return
  const m = t.getBoundingClientRect()
  size.value = fitFontSize(b.clientWidth, b.clientHeight, m.width, m.height, PROBE)
}

let resized: ResizeObserver | undefined
onMounted(() => {
  fit()
  resized = new ResizeObserver(fit)
  if (box.value) resized.observe(box.value)
  void document.fonts?.ready.then(fit)
})
onBeforeUnmount(() => resized?.disconnect())
watch(() => [props.widget.text, props.widget.unit], fit, { flush: 'post' })
</script>

<template>
  <div class="w-display" :data-status="widget.status ?? undefined">
    <div v-if="widget.label" class="w-label">{{ widget.label }}</div>
    <div ref="box" class="dp-box">
      <span class="dp-text" :style="{ fontSize: size + 'px' }">{{ widget.text }}<small v-if="widget.unit">{{ widget.unit }}</small></span>
      <span ref="probe" class="dp-text dp-probe" aria-hidden="true" :style="{ fontSize: PROBE + 'px' }">{{ widget.text }}<small v-if="widget.unit">{{ widget.unit }}</small></span>
    </div>
    <div v-if="widget.caption" class="dp-caption">{{ widget.caption }}</div>
  </div>
</template>
