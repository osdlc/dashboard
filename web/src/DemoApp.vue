<script setup lang="ts">
import { nextTick, onMounted, ref } from 'vue'
import type { CatalogEntry, Size, TileUpdate } from './contract'
import TileCard from './components/TileCard.vue'
import { applyColorTheme, builtinTheme } from './colors'

/**
 * Demo mode (`?mode=demo`, see mode.ts): catalog entries' demo views drawn
 * as tiles, with no server. With `id`, one tile at the width its size has
 * on a 1200px grid, for screenshots: the entry's default size, or `size`
 * (small, medium, large, wide or tall); without, every entry. With `id`
 * and `fill`, the tile fills the page instead, for a frame the reader
 * resizes (the site's widget pages embed it so).
 * `theme` (light or dark) fixes the mode and `color_theme` picks a built-in
 * color theme by name.
 * `data-ready` is set on <html> once the tiles are drawn. A `{ theme,
 * color_theme }` message from the embedding page (same origin) switches them.
 */
const params = new URLSearchParams(location.search)
const id = params.get('id')
const SIZES: Size[] = ['small', 'medium', 'large', 'wide', 'tall']
const asked = params.get('size')
const size = SIZES.find((s) => s === asked)
const fill = id !== null && params.has('fill')
const root = document.documentElement
const setTheme = (t: unknown) => {
  if (t === 'light' || t === 'dark') root.dataset.theme = t
}
const setColors = (name: unknown) => {
  const colors = typeof name === 'string' ? builtinTheme(name) : null
  if (colors) applyColorTheme(colors)
}
setTheme(params.get('theme'))
setColors(params.get('color_theme'))
root.dataset.mode = 'demo'
if (fill) root.dataset.fill = ''
window.addEventListener('message', (e) => {
  if (e.origin !== location.origin) return
  const data = e.data as { theme?: unknown; color_theme?: unknown } | null
  setTheme(data?.theme)
  setColors(data?.color_theme)
})

const entries = ref<CatalogEntry[]>([])
const error = ref<string | null>(null)

/** The demo view, shaped like a live tile so the usual components draw it. */
const sizeOf = (e: CatalogEntry): Size => (id && size) || e.default_size
const tile = (e: CatalogEntry): TileUpdate => ({ tile_id: e.id, view: e.demo, stale: false, demo: false, updated_at: 0 })

onMounted(async () => {
  try {
    // Only a path on this server: a catalog from elsewhere could put any link in a tile.
    const catalog = params.get('catalog') ?? 'catalog.json'
    if (/^[a-z][a-z0-9+.-]*:|^[\\/]{2}/i.test(catalog)) throw new Error(`catalog: not a local path: ${catalog}`)
    const res = await fetch(catalog)
    if (!res.ok) throw new Error(`catalog: HTTP ${res.status}`)
    const all = ((await res.json()) as { entries: CatalogEntry[] }).entries
    entries.value = id ? all.filter((e) => e.id === id) : all
    if (!entries.value.length) throw new Error(`no catalog entry ${id}`)
    if (asked && !size) throw new Error(`size: not one of ${SIZES.join(', ')}: ${asked}`)
  } catch (err) {
    error.value = String(err)
  }
  await nextTick()
  await document.fonts.ready
  root.dataset.ready = error.value ? 'error' : 'ok'
})
</script>

<template>
  <p v-if="error" class="notice">{{ error }}</p>
  <main v-else-if="id" :class="fill ? 'demo-fill' : 'demo-one'">
    <TileCard v-for="e in entries" :key="e.id" :tile-id="e.id" :size="sizeOf(e)" :tile="tile(e)" />
  </main>
  <main v-else class="grid">
    <TileCard v-for="e in entries" :key="e.id" :tile-id="e.id" :size="e.default_size" :tile="tile(e)" />
  </main>
</template>
