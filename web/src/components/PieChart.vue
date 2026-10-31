<script setup lang="ts">
import { computed } from 'vue'
import { ariaLabel, layout, type Pie } from '../pie'

const props = defineProps<{ widget: Pie }>()
const slices = computed(() => layout(props.widget))
const label = computed(() => ariaLabel(props.widget, slices.value))
</script>

<template>
  <div class="w-pie">
    <div v-if="widget.label" class="w-label">{{ widget.label }}</div>
    <p v-if="slices.length === 0" class="pie-empty">No data</p>
    <div v-else class="pie-body">
      <svg class="pie-disc" viewBox="0 0 100 100" role="img" :aria-label="label">
        <title>{{ label }}</title>
        <template v-for="(s, i) in slices" :key="i">
          <path v-if="s.path" :d="s.path" :fill="s.color" fill-rule="evenodd" />
        </template>
      </svg>
      <ul class="pie-legend" aria-hidden="true">
        <li v-for="(s, i) in slices" :key="i">
          <span class="p-swatch" :style="{ background: s.color }" />
          <span class="p-label" :title="s.label">{{ s.label }}</span>
          <span class="p-value">{{ s.value }}</span>
          <span class="p-pct">{{ s.percent }}</span>
        </li>
      </ul>
    </div>
  </div>
</template>
