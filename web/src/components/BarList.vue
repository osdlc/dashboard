<script setup lang="ts">
import { computed } from 'vue'
import type { Widget } from '../contract'
import { formatValue } from '../format'
import { stableKeys } from '../motion'

type Bars = Extract<Widget, { type: 'bars' }>
const props = defineProps<{ widget: Bars }>()
const keys = computed(() => stableKeys(props.widget.bars, (b) => b.label))
const max = computed(() => props.widget.max ?? Math.max(Number.EPSILON, ...props.widget.bars.map((b) => b.value)))
</script>

<template>
  <div class="w-bars">
    <div v-if="widget.label" class="w-label">{{ widget.label }}</div>
    <TransitionGroup name="row">
    <div v-for="(b, i) in widget.bars" :key="keys[i]" class="bar-row" :data-status="b.status ?? undefined" data-nav-item>
      <span class="b-label" :title="b.label">{{ b.label }}</span>
      <span class="b-track"><span class="b-fill" :style="{ width: Math.min(100, (b.value / max) * 100) + '%' }" /></span>
      <span class="b-value">{{ formatValue(b.value, widget.format, widget.unit) }}</span>
    </div>
    </TransitionGroup>
  </div>
</template>
