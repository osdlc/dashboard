<script setup lang="ts">
import type { UptimeCheck, Widget } from '../contract'

type Uptime = Extract<Widget, { type: 'uptime' }>
defineProps<{ widget: Uptime }>()

/** Most results that fit in a wide tile; older ones are clipped on the left anyway. */
const SHOWN = 90

function percent(c: UptimeCheck): string {
  if (c.uptime == null) return ''
  return c.uptime >= 100 ? '100%' : `${c.uptime.toFixed(1)}%`
}

/** The newest results, padded on the left with empty cells so the strip starts full width. */
function cells(c: UptimeCheck): Array<UptimeCheck['status'] | null> {
  const h = (c.history ?? []).slice(-SHOWN)
  return [...Array<null>(SHOWN - h.length).fill(null), ...h]
}

/** The time when the check passes, otherwise what went wrong. */
function last(c: UptimeCheck): string {
  if (c.status !== 'ok' && c.detail) return c.detail
  if (c.latency_ms == null) return ''
  return c.latency_ms < 1000 ? `${c.latency_ms.toFixed(0)}ms` : `${(c.latency_ms / 1000).toFixed(1)}s`
}
</script>

<template>
  <div class="w-uptime">
    <div v-for="c in widget.checks" :key="c.name" class="up-row" :data-status="c.status" data-nav-item>
      <span class="up-dot" />
      <a v-if="c.url" class="up-name" :href="c.url" target="_blank" rel="noopener">{{ c.name }}</a>
      <span v-else class="up-name">{{ c.name }}</span>
      <span class="up-strip" :title="`Last ${Math.min(c.history?.length ?? 0, SHOWN)} checks, newest on the right`">
        <span v-for="(s, i) in cells(c)" :key="i" class="up-cell" :data-status="s ?? undefined" />
      </span>
      <span class="up-pct">{{ percent(c) }}</span>
      <span class="up-last" :title="c.detail ?? undefined">{{ last(c) }}</span>
    </div>
  </div>
</template>
