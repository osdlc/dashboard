<script setup lang="ts">
import type { Widget } from '../contract'
import { scaleColor } from '../scale'
import { toneColor } from '../colors'

type Grid = Extract<Widget, { type: 'meter_grid' }>
defineProps<{ widget: Grid }>()
</script>

<template>
  <div class="w-meters">
    <div v-if="widget.label" class="w-label">{{ widget.label }}</div>
    <div class="meters">
      <div v-for="m in widget.meters" :key="m.label" class="meter">
        <span class="m-label">{{ m.label }}</span>
        <span class="m-track">
          <span
            class="m-fill"
            :style="{
              width: Math.min(100, Math.max(0, m.percent)) + '%',
              background: m.tone ? toneColor(m.tone) : scaleColor(widget.color, m.percent / 100),
            }"
          />
        </span>
        <span class="m-value">{{ m.percent.toFixed(0) }}</span>
      </div>
    </div>
  </div>
</template>
