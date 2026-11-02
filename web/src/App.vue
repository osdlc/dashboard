<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { runAction } from './actions'
import { addSeq, isOutcomeOf, type PendingAdd } from './adds'
import type { ClientMessage, ScreenLayout } from './contract'
import HelpPanel from './components/HelpPanel.vue'
import ScreenEditor from './components/ScreenEditor.vue'
import SettingsPanel from './components/SettingsPanel.vue'
import TileCard from './components/TileCard.vue'
import { useDensity } from './density'
import { NO_FOCUS, back, dirForKey, enter, focusedTile, items, moved, type Box, type Focus } from './nav'
import { useDashboard } from './useDashboard'
import { useRotation } from './useRotation'
import { useScreens } from './useScreens'

const { tiles, settings, state, send } = useDashboard()
const { screens, index, current, go } = useScreens(settings)
const overlay = ref<'none' | 'help' | 'settings'>('none')
const toggle = (o: 'help' | 'settings') => (overlay.value = overlay.value === o ? 'none' : o)
const rotation = useRotation(settings, index, go)
const density = useDensity()
const editing = ref(false)
/** Tile shown fullscreen after a double-click. */
const expanded = ref<string | null>(null)
const rotationText = { on: '⟳ rotating', paused: '⟳ paused', off: '⟳ off', none: '' } as const
/** Ambient mode (`?mode=ambient`) is for looking at, not using: no keyboard focus there. */
const ambient = new URLSearchParams(location.search).get('mode') === 'ambient'

/** Keyboard focus: a tile, or an item inside one. See nav.ts. */
const focus = ref<Focus>(NO_FOCUS)
watch(() => current.value?.name, () => (focus.value = NO_FOCUS))

/** The tiles the arrow keys move between, where they are on the page. */
function tileBoxes(): Array<[string, Box]> {
  const scope = expanded.value ? '.fullscreen' : 'main.grid'
  return [...document.querySelectorAll<HTMLElement>(`${scope} .tile[data-tile-id]`)].map((el) => {
    const r = el.getBoundingClientRect()
    return [el.dataset.tileId!, { x: r.left, y: r.top, width: r.width, height: r.height }]
  })
}

function focusedItems() {
  const id = focusedTile(focus.value)
  const tile = id ? tiles.value[id] : undefined
  return tile ? items(tile.view.root) : []
}

/** Moves browser focus to the focused tile, so screen readers follow along. */
async function setFocus(f: Focus) {
  focus.value = f
  await nextTick()
  const id = focusedTile(f)
  const scope = expanded.value ? '.fullscreen' : 'main.grid'
  const el = id ? document.querySelector<HTMLElement>(`${scope} .tile[data-tile-id="${CSS.escape(id)}"]`) : null
  if (el) el.focus({ preventScroll: true })
  else if (document.activeElement instanceof HTMLElement && document.activeElement.closest('.tile')) {
    document.activeElement.blur()
  }
}

/** Clicking or tabbing into a tile focuses it (keeping an item selected in it). */
function onTileFocus(id: string) {
  if (focusedTile(focus.value) !== id) focus.value = { kind: 'tile', tile: id }
}

/** Arrow keys, Enter and Esc for tile focus. Returns true when the key was used. */
function navKey(e: KeyboardEvent): boolean {
  const dir = dirForKey(e.key)
  if (dir) {
    // No tiles: leave the arrows to scroll the page.
    const boxes = tileBoxes()
    if (boxes.length === 0) return false
    void setFocus(moved(focus.value, dir, boxes, focusedItems()))
    return true
  }
  // Enter on a button or link does its own thing.
  if (e.key === 'Enter' && !(e.target instanceof HTMLButtonElement || e.target instanceof HTMLAnchorElement)) {
    if (focus.value.kind === 'none') return false
    const [f, action] = enter(focus.value, focusedItems())
    void setFocus(f)
    if (action) runAction(action)
    return true
  }
  if (e.key === 'Escape' && focus.value.kind === 'item') {
    void setFocus(back(focus.value))
    return true
  }
  return false
}

/** Keys here must match keys.ts. */
function onKey(e: KeyboardEvent) {
  if (e.metaKey || e.ctrlKey || e.altKey || e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return
  if (editing.value) return // the editor handles its own keys
  // Keyboard focus is off while help or settings are open.
  if (!ambient && overlay.value === 'none' && navKey(e)) {
    e.preventDefault()
    rotation.input()
    return
  }
  if (expanded.value) {
    if (e.key === 'Escape') expanded.value = null
    return
  }
  if (e.key === 'r' && rotation.state.value !== 'none') return rotation.toggle()
  rotation.input()
  if (e.key === '?') toggle('help')
  else if (e.key === 's') toggle('settings')
  else if (e.key === 'e' && settings.value) openEditor()
  else if (e.key === 'd') density.toggle()
  else if (e.key === 'Escape' && overlay.value === 'none') void setFocus(back(focus.value))
  else if (e.key === 'Escape') overlay.value = 'none'
  else if (e.key === ']') go(index.value + 1)
  else if (e.key === '[') go(index.value - 1)
  else if (/^[1-9]$/.test(e.key) && Number(e.key) <= screens.value.length) go(Number(e.key) - 1)
}
/** The config version the screen editor started from: a save over newer changes is refused. */
let editBase: string | null = null
function openEditor() {
  overlay.value = 'none'
  rotation.input()
  editBase = settings.value?.config.etag ?? null
  editing.value = true
}

/** The switcher's value: a saved dashboard's name, or '' for config.yaml alone. */
function switchDashboard(e: Event) {
  const select = e.target as HTMLSelectElement
  const name = select.value
  // Keep showing the current one until the server sends new settings, so a
  // switch that fails doesn't leave the other name showing.
  select.value = settings.value?.config.dashboard ?? ''
  send({ type: 'switch_dashboard', name: name || null })
}

/** An add sent from the editor, and whether the editor's base was the file as it was then. */
let pendingAdd: (PendingAdd & { onBase: boolean }) | null = null
function addModule(msg: ClientMessage) {
  if (msg.type === 'add_module') {
    pendingAdd = {
      moduleId: msg.module_id,
      before: addSeq(settings.value?.config.added),
      onBase: settings.value?.config.etag === editBase,
    }
  }
  send(msg)
}
// The add wrote the file and the editor puts its tile in the draft, so a save
// builds on the new version. Not when the file had already changed under the editor.
watch(
  () => settings.value?.config,
  (c) => {
    if (!c || !pendingAdd || !isOutcomeOf(c.added, pendingAdd)) return
    if (!c.added.error && pendingAdd.onBase) editBase = c.etag ?? null
    pendingAdd = null
  },
)

function saveScreens(edited: ScreenLayout[]) {
  send({ type: 'save_screens', screens: edited, etag: editBase })
  editing.value = false
}

onMounted(() => window.addEventListener('keydown', onKey))
onUnmounted(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <header class="top">
    <h1>Dashboard</h1>
    <select
      v-if="settings && (settings.config.dashboards?.length || settings.config.dashboard)"
      class="dash-switch"
      aria-label="Dashboard"
      title="Switch dashboard (saved in ~/.config/dashboard/dashboards)"
      :value="settings.config.dashboard ?? ''"
      @change="switchDashboard"
    >
      <option value="">config.yaml</option>
      <option v-for="d in settings.config.dashboards" :key="d.name" :value="d.name" :title="d.description ?? undefined">
        {{ d.name }}
      </option>
    </select>
    <nav class="tabs" aria-label="Screens">
      <button
        v-for="(s, i) in screens"
        :key="s.name"
        type="button"
        :aria-current="i === index ? 'page' : undefined"
        @click="go(i)"
      >
        {{ i + 1 }} {{ s.name }}
      </button>
    </nav>
    <span class="conn" :data-state="state">{{ state }}</span>
    <button
      v-if="rotation.state.value !== 'none'"
      type="button"
      class="rotate-btn"
      :data-state="rotation.state.value"
      title="Start or stop screen rotation (r)"
      @click="rotation.toggle()"
    >
      {{ rotationText[rotation.state.value] }}
    </button>
    <button type="button" class="help-btn edit-btn" title="Edit screens (e)" :disabled="!settings" @click="openEditor">
      ✎ Edit
    </button>
    <button
      type="button"
      class="help-btn"
      :title="`Switch to ${density.density.value === 'compact' ? 'comfortable' : 'compact'} spacing (d)`"
      @click="density.toggle()"
    >
      {{ density.density.value === 'compact' ? '▦ Compact' : '▢ Roomy' }}
    </button>
    <button type="button" class="help-btn settings-btn" aria-label="Settings" title="Settings (s)" @click="toggle('settings')">⚙</button>
    <button type="button" class="help-btn" aria-label="Help" title="Help (?)" @click="toggle('help')">?</button>
  </header>
  <p v-if="state === 'signed out'" class="notice">
    Not signed in. A sign-in link works once and for a minute; run <code>dashboard open</code> for a new
    one.
  </p>
  <ScreenEditor
    v-if="editing && settings"
    :settings="settings"
    :tiles="tiles"
    :start="index"
    @save="saveScreens"
    @add="addModule"
    @close="editing = false"
  />
  <main v-else-if="current" :key="current.name" class="grid">
    <TileCard
      v-for="t in current.tiles"
      :key="t.tile_id"
      :tile-id="t.tile_id"
      :size="t.size"
      :tile="tiles[t.tile_id]"
      :focus="ambient ? undefined : focus"
      :tabbable="focus.kind === 'none' && t === current.tiles[0]"
      title="Double-click to expand"
      @dblclick="expanded = t.tile_id"
      @focusin="onTileFocus(t.tile_id)"
    />
    <p v-if="current.tiles.length === 0" class="notice">This screen has no running modules.</p>
  </main>
  <div v-if="expanded" class="overlay fullscreen" @click.self="expanded = null">
    <TileCard
      :tile-id="expanded"
      size="full"
      :tile="tiles[expanded]"
      :focus="ambient ? undefined : focus"
      @dblclick="expanded = null"
      @focusin="onTileFocus(expanded)"
    >
      <button type="button" class="close-full" aria-label="Close" title="Close (Esc)" @click="expanded = null">✕</button>
    </TileCard>
  </div>
  <HelpPanel v-if="overlay === 'help' && settings" :settings="settings" :current="index" @close="overlay = 'none'" />
  <SettingsPanel
    v-if="overlay === 'settings' && settings"
    :settings="settings"
    @close="overlay = 'none'"
    @send="send"
  />
  <p v-if="settings?.config.error && overlay !== 'settings'" class="config-banner" role="alert">
    Config not applied: {{ settings.config.error.split('\n')[0] }}
    <button type="button" @click="overlay = 'settings'">Details</button>
  </p>
</template>
