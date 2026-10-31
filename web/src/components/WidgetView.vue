<script setup lang="ts">
import { computed } from 'vue'
import type { Cell, Thresholds, Widget } from '../contract'
import { inlineMarkdown } from '../format'
import { webUrl } from '../actions'
import { stableKeys } from '../motion'
import { toneColor } from '../colors'
import AnimatedNumber from './AnimatedNumber.vue'
import BarList from './BarList.vue'
import UptimeView from './UptimeView.vue'
import ImageView from './ImageView.vue'
import LogView from './LogView.vue'
import MeterGrid from './MeterGrid.vue'
import PieChart from './PieChart.vue'
import SeriesChart from './SeriesChart.vue'

defineOptions({ name: 'WidgetView' })
const props = defineProps<{ widget: Widget }>()

function level(percent: number, t?: Thresholds | null): string {
  if (!t) return 'ok'
  return percent >= t.error ? 'error' : percent >= t.warn ? 'warn' : 'ok'
}

function isWidget(cell: Cell | undefined): cell is Widget {
  return typeof cell === 'object' && cell !== null
}

const w = computed(() => props.widget)

/** Keys for list items and table rows that survive updates, for move/enter animations. */
const keys = computed(() => {
  const x = w.value
  if (x.type === 'list') return stableKeys(x.items, (i) => i.title)
  if (x.type === 'table') {
    const first = x.columns[0]?.key ?? ''
    return stableKeys(x.rows, (r) => String(r.cells[first] ?? ''))
  }
  return []
})
</script>

<template>
  <p v-if="w.type === 'text'" class="w-text">
    <template v-if="w.markdown">
      <template v-for="(part, i) in inlineMarkdown(w.text)" :key="i">
        <strong v-if="part.kind === 'strong'">{{ part.text }}</strong>
        <code v-else-if="part.kind === 'code'">{{ part.text }}</code>
        <template v-else>{{ part.text }}</template>
      </template>
    </template>
    <template v-else>{{ w.text }}</template>
  </p>

  <div v-else-if="w.type === 'stat'" class="w-stat" :data-status="w.status ?? undefined">
    <span class="label">{{ w.label }}</span>
    <span class="value"><AnimatedNumber :value="w.value" /><small v-if="w.unit"> {{ w.unit }}</small></span>
    <span v-if="w.delta" class="delta">{{ w.delta }}</span>
  </div>

  <div v-else-if="w.type === 'gauge'" class="w-gauge" :data-level="level(w.percent, w.thresholds)">
    <div class="gauge-head">
      <span>{{ w.label }}</span>
      <span>{{ w.percent.toFixed(0) }}%</span>
    </div>
    <div class="bar">
      <div class="fill" :style="{ width: Math.min(100, Math.max(0, w.percent)) + '%' }" />
      <div v-if="w.marker != null" class="marker" :style="{ left: w.marker + '%' }" />
    </div>
    <div v-if="w.caption" class="gauge-caption">{{ w.caption }}</div>
  </div>

  <SeriesChart v-else-if="w.type === 'series'" :widget="w" />

  <MeterGrid v-else-if="w.type === 'meter_grid'" :widget="w" />

  <BarList v-else-if="w.type === 'bars'" :widget="w" />

  <PieChart v-else-if="w.type === 'pie'" :widget="w" />

  <LogView v-else-if="w.type === 'log'" :widget="w" />

  <UptimeView v-else-if="w.type === 'uptime'" :widget="w" />

  <ImageView v-else-if="w.type === 'image'" :widget="w" />
  <TransitionGroup v-else-if="w.type === 'list'" tag="ul" name="row" class="w-list">
    <li v-if="w.items.length === 0" key="__empty" class="empty">{{ w.empty_text ?? 'Nothing here' }}</li>
    <li
      v-for="(item, i) in w.items"
      :key="keys[i]"
      :data-status="item.status ?? undefined"
      :data-tone="!item.status && item.tone ? item.tone : undefined"
      :style="!item.status && item.tone ? { '--tone': toneColor(item.tone) } : undefined"
      data-nav-item
    >
      <div class="item-head">
        <a v-if="item.action?.type === 'open_url' && webUrl(item.action.url)" class="title" :title="item.title" :href="webUrl(item.action.url)" target="_blank" rel="noopener">{{ item.title }}</a>
        <span v-else class="title" :title="item.title">{{ item.title }}</span>
        <span v-for="tag in item.tags ?? []" :key="tag" class="tag">{{ tag }}</span>
      </div>
      <div v-if="item.subtitle" class="sub" :title="item.subtitle">{{ item.subtitle }}</div>
    </li>
  </TransitionGroup>

  <table v-else-if="w.type === 'table'" class="w-table">
    <thead>
      <tr><th v-for="c in w.columns" :key="c.key" :data-priority="c.priority ?? undefined">{{ c.label }}</th></tr>
    </thead>
    <TransitionGroup tag="tbody" name="row">
      <tr v-for="(row, i) in w.rows" :key="keys[i]" :data-status="row.status ?? undefined" data-nav-item>
        <td v-for="c in w.columns" :key="c.key">
          <WidgetView v-if="isWidget(row.cells[c.key])" :widget="row.cells[c.key] as Widget" />
          <span v-else :title="String(row.cells[c.key] ?? '')">{{ row.cells[c.key] ?? '' }}</span>
        </td>
      </tr>
    </TransitionGroup>
  </table>

  <div v-else-if="w.type === 'row'" class="w-row">
    <WidgetView v-for="(child, i) in w.children" :key="i" :widget="child" />
  </div>

  <div v-else-if="w.type === 'column'" class="w-column">
    <WidgetView v-for="(child, i) in w.children" :key="i" :widget="child" />
  </div>
</template>
