<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, useId, watch } from 'vue'
import type { TilePlacement, TileUpdate } from '../contract'
import { fitTile } from '../fit'
import { reducedMotion } from '../motion'
import { focusedTile, itemIndex, items, type Focus } from '../nav'
import WidgetView from './WidgetView.vue'

/**
 * `full` is the double-click fullscreen view. `focus` is the keyboard focus,
 * left out where there is no keyboard navigation (the screen editor).
 * `tabbable` makes this the tile Tab lands on when none is focused.
 */
const props = defineProps<{
  tileId: string
  size: TilePlacement['size'] | 'full'
  tile: TileUpdate | undefined
  focus?: Focus | undefined
  tabbable?: boolean
}>()

const root = ref<HTMLElement | null>(null)
const uid = useId()
const navItems = computed(() => (props.tile && props.focus ? items(props.tile.view.root) : []))
const focused = computed(() => !!props.focus && focusedTile(props.focus) === props.tileId)
const selected = computed(() => (focused.value && props.focus ? itemIndex(props.focus, navItems.value) : null))
/** The selected item's element id, for aria-activedescendant. */
const active = ref<string | undefined>()

/**
 * Marks the selected item. Widgets tag their items with `data-nav-item`, in
 * the same order as `items()`. Runs after every update, since rows move.
 */
function mark() {
  if (!props.focus) return
  // Rows still fading out after an update are not items any more, and a
  // widget inside a table cell is not counted by items().
  const all = root.value?.querySelectorAll<HTMLElement>('[data-nav-item]:not(.row-leave-active)') ?? []
  const els = [...all].filter((el) => !el.parentElement?.closest('[data-nav-item]'))
  els.forEach((el, i) => {
    el.id = `${uid}-item-${i}`
    el.setAttribute('role', 'option')
    el.setAttribute('aria-selected', String(i === selected.value))
  })
  active.value = selected.value === null ? undefined : els[selected.value]?.id
}
watch([selected, () => props.tile], mark, { flush: 'post', immediate: true })

/**
 * Fits the view to the tile's box (fit.ts) after every update and whenever
 * the tile changes size. The tile's size comes from the grid, not from
 * what is cut inside it, so fitting never triggers itself.
 */
function fit() {
  if (root.value) fitTile(root.value)
}
let resized: ResizeObserver | undefined
onMounted(() => {
  resized = new ResizeObserver(fit)
  if (root.value) resized.observe(root.value)
  void document.fonts?.ready.then(fit)
})
onBeforeUnmount(() => resized?.disconnect())
watch([() => props.tile, () => props.size], fit, { flush: 'post' })

// Scroll the selection into view when it moves, not on every data update.
watch(
  () => props.focus,
  () => {
    if (!focused.value) return
    const el = (active.value && document.getElementById(active.value)) || root.value
    el?.scrollIntoView({ block: 'nearest', behavior: reducedMotion() ? 'auto' : 'smooth' })
  },
  { flush: 'post' },
)
</script>

<template>
  <section
    ref="root"
    class="tile"
    :data-size="size"
    :data-status="tile?.view.status ?? 'unknown'"
    :data-tile-id="tileId"
    :data-focused="focused || undefined"
    :tabindex="focus ? (tabbable || focused ? 0 : -1) : undefined"
    :role="focus ? (navItems.length ? 'listbox' : 'group') : undefined"
    :aria-label="focus ? tileId : undefined"
    :aria-activedescendant="active"
  >
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
