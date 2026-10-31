<script setup lang="ts">
import TileCard from './components/TileCard.vue'
import { useDashboard } from './useDashboard'
import { useRotation } from './useRotation'
import { useScreens } from './useScreens'

/** Ambient mode (`?mode=ambient`, see mode.ts): the current screen's tiles only, rotating. */
const { tiles, settings, state } = useDashboard()
const { screens, index, current, go } = useScreens(settings)
useRotation(settings, index, go, true)
document.documentElement.dataset.mode = 'ambient'
</script>

<template>
  <!-- inert: nothing in ambient mode takes focus or clicks, not even with Tab. -->
  <main v-if="current" :key="current.name" class="grid" aria-live="off" inert>
    <TileCard v-for="t in current.tiles" :key="t.tile_id" :tile-id="t.tile_id" :size="t.size" :tile="tiles[t.tile_id]" />
  </main>
  <footer class="ambient-foot">
    <span v-if="screens.length > 1">{{ current?.name }} · {{ index + 1 }}/{{ screens.length }}</span>
    <span v-if="state !== 'open'" class="conn" :data-state="state">{{ state }}</span>
  </footer>
</template>
