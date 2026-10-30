<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import type { ScreenLayout, TilePlacement, TileUpdate, UiSettings } from '../contract'
import TileCard from './TileCard.vue'

type Size = TilePlacement['size']
const SIZES: ReadonlyArray<readonly [Size, string]> = [
  ['small', 'Small · 3×1'],
  ['medium', 'Medium · 4×2'],
  ['large', 'Large · 6×3'],
  ['wide', 'Wide · 8×2'],
  ['tall', 'Tall · 4×4'],
]

const props = defineProps<{ settings: UiSettings; tiles: Record<string, TileUpdate>; start: number }>()
const emit = defineEmits<{ save: [screens: ScreenLayout[]]; close: [] }>()

// Work on a copy; nothing changes until Save.
const draft = ref<ScreenLayout[]>(JSON.parse(JSON.stringify(props.settings.screens)))
const selected = ref(Math.min(props.start, Math.max(0, draft.value.length - 1)))
const screen = computed(() => draft.value[selected.value])
const dirty = computed(() => JSON.stringify(draft.value) !== JSON.stringify(props.settings.screens))
const error = computed(() => {
  const names = draft.value.map((s) => s.name.trim())
  if (names.some((n) => !n)) return 'Every screen needs a name.'
  if (new Set(names).size !== names.length) return 'Screen names must be different.'
  return ''
})

const modules = computed(() => [...props.settings.modules].sort((a, b) => a.tile_id.localeCompare(b.tile_id)))
const onScreen = computed(() => new Set(screen.value?.tiles.map((t) => t.tile_id)))

function addScreen() {
  let n = draft.value.length + 1
  while (draft.value.some((s) => s.name === `screen ${n}`)) n++
  draft.value.push({ name: `screen ${n}`, tiles: [] })
  selected.value = draft.value.length - 1
}

function removeScreen() {
  draft.value.splice(selected.value, 1)
  selected.value = Math.max(0, selected.value - 1)
}

function addTile(tileId: string) {
  const m = props.settings.modules.find((x) => x.tile_id === tileId)
  screen.value?.tiles.push({ tile_id: tileId, size: m?.default_size ?? 'medium' })
}

function move(from: number, to: number) {
  const tiles = screen.value?.tiles
  if (!tiles || to < 0 || to >= tiles.length) return
  const [t] = tiles.splice(from, 1)
  if (t) tiles.splice(to, 0, t)
}

// ---- right-click menu ----
const menu = ref<{ index: number; x: number; y: number } | null>(null)
function openMenu(index: number, e: MouseEvent) {
  // Keep the menu on screen near the pointer.
  menu.value = { index, x: Math.min(e.clientX, window.innerWidth - 200), y: Math.min(e.clientY, window.innerHeight - 280) }
}
function setSize(size: Size) {
  const t = menu.value && screen.value?.tiles[menu.value.index]
  if (t) t.size = size
  menu.value = null
}
function menuMove(step: number) {
  if (menu.value) move(menu.value.index, menu.value.index + step)
  menu.value = null
}
function menuRemove() {
  if (menu.value) screen.value?.tiles.splice(menu.value.index, 1)
  menu.value = null
}

// ---- drag to reorder ----
const dragging = ref<number | null>(null)
function drop(to: number) {
  if (dragging.value !== null) move(dragging.value, to)
  dragging.value = null
}

function save() {
  if (error.value) return
  emit(
    'save',
    draft.value.map((s) => ({ ...s, name: s.name.trim() })),
  )
}

function onKey(e: KeyboardEvent) {
  if (e.key !== 'Escape') return
  e.stopPropagation()
  if (menu.value) menu.value = null
  else emit('close')
}
onMounted(() => window.addEventListener('keydown', onKey, true))
onUnmounted(() => window.removeEventListener('keydown', onKey, true))
</script>

<template>
  <div class="editor" @click="menu = null">
    <div class="editor-bar">
      <strong>Edit screens</strong>
      <nav class="tabs" aria-label="Screens being edited">
        <button
          v-for="(s, i) in draft"
          :key="i"
          type="button"
          :aria-current="i === selected ? 'page' : undefined"
          @click="selected = i"
        >
          {{ s.name || '(unnamed)' }}
        </button>
        <button type="button" class="add" title="Create a screen" @click="addScreen">+ New screen</button>
      </nav>
      <span class="spacer" />
      <span v-if="error" class="editor-error">{{ error }}</span>
      <button type="button" @click="emit('close')">Cancel</button>
      <button type="button" class="primary" :disabled="!dirty || !!error" @click="save">Save</button>
    </div>

    <div v-if="screen" class="editor-screen-bar">
      <label>
        Name
        <input v-model="screen.name" type="text" spellcheck="false" />
      </label>
      <button type="button" :disabled="draft.length <= 1" title="Delete this screen" @click="removeScreen">
        Delete screen
      </button>
      <span class="muted">Right-click a tile to resize, move or remove it. Drag tiles to reorder.</span>
    </div>

    <div class="editor-body">
      <main v-if="screen" class="grid editing">
        <TileCard
          v-for="(t, i) in screen.tiles"
          :key="`${t.tile_id}-${i}`"
          :tile-id="t.tile_id"
          :size="t.size"
          :tile="tiles[t.tile_id]"
          draggable="true"
          :data-dragging="dragging === i || undefined"
          @contextmenu.prevent.stop="openMenu(i, $event)"
          @dragstart="dragging = i"
          @dragend="dragging = null"
          @dragover.prevent
          @drop.prevent="drop(i)"
        >
          <button type="button" class="size-badge" title="Size and more" @click.stop="openMenu(i, $event)">
            {{ t.size }} ▾
          </button>
        </TileCard>
        <p v-if="screen.tiles.length === 0" class="notice">Empty screen. Add components from the list on the right.</p>
      </main>

      <aside class="palette" aria-label="Add a component">
        <h3>Add component</h3>
        <button
          v-for="m in modules"
          :key="m.tile_id"
          type="button"
          class="palette-item"
          :data-used="onScreen.has(m.tile_id) || undefined"
          :title="`Add ${m.tile_id} (${m.default_size})`"
          @click="addTile(m.tile_id)"
        >
          <span>{{ m.tile_id }}</span>
          <small>{{ m.name }} · {{ m.default_size }}</small>
        </button>
      </aside>
    </div>

    <div v-if="menu" class="ctx-menu" :style="{ left: `${menu.x}px`, top: `${menu.y}px` }" role="menu" @click.stop>
      <div class="ctx-label">Size</div>
      <button
        v-for="[size, label] in SIZES"
        :key="size"
        type="button"
        role="menuitemradio"
        :aria-checked="screen?.tiles[menu.index]?.size === size"
        @click="setSize(size)"
      >
        {{ label }}
      </button>
      <hr />
      <button type="button" role="menuitem" :disabled="menu.index === 0" @click="menuMove(-1)">Move earlier</button>
      <button
        type="button"
        role="menuitem"
        :disabled="menu.index >= (screen?.tiles.length ?? 0) - 1"
        @click="menuMove(1)"
      >
        Move later
      </button>
      <hr />
      <button type="button" role="menuitem" class="danger" @click="menuRemove">Remove from screen</button>
    </div>
  </div>
</template>
