<script setup lang="ts">
import type { TilePlacement, TileUpdate } from '../contract'
import WidgetView from './WidgetView.vue'

/** `full` is the double-click fullscreen view. */
defineProps<{ tileId: string; size: TilePlacement['size'] | 'full'; tile: TileUpdate | undefined }>()
</script>

<template>
  <section class="tile" :data-size="size" :data-status="tile?.view.status ?? 'unknown'">
    <header>
      <h2>{{ tileId }}</h2>
      <span v-if="tile?.stale" class="stale">stale</span>
      <span v-if="tile?.demo" class="demo" title="Made-up data: this module has demo: true in the config">demo</span>
      <span class="summary">{{ tile?.view.summary ?? 'waiting…' }}</span>
    </header>
    <WidgetView v-if="tile" :widget="tile.view.root" />
    <slot />
  </section>
</template>
