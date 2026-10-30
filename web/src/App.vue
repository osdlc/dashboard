<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import type { ScreenLayout } from './contract'
import HelpPanel from './components/HelpPanel.vue'
import ScreenEditor from './components/ScreenEditor.vue'
import SettingsPanel from './components/SettingsPanel.vue'
import TileCard from './components/TileCard.vue'
import { useDensity } from './density'
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

/** Keys here must match keys.ts. */
function onKey(e: KeyboardEvent) {
  if (e.metaKey || e.ctrlKey || e.altKey || e.target instanceof HTMLInputElement) return
  if (editing.value) return // the editor handles its own keys
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
  else if (e.key === 'Escape') overlay.value = 'none'
  else if (e.key === ']') go(index.value + 1)
  else if (e.key === '[') go(index.value - 1)
  else if (/^[1-9]$/.test(e.key) && Number(e.key) <= screens.value.length) go(Number(e.key) - 1)
}
function openEditor() {
  overlay.value = 'none'
  rotation.input()
  editing.value = true
}

function saveScreens(edited: ScreenLayout[]) {
  send({ type: 'save_screens', screens: edited })
  editing.value = false
}

onMounted(() => window.addEventListener('keydown', onKey))
onUnmounted(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <header class="top">
    <h1>Dashboard</h1>
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
  <p v-if="state === 'no-token'" class="notice">
    Open this page with the <code>?token=…</code> link printed by <code>dashboard serve</code>.
  </p>
  <ScreenEditor
    v-if="editing && settings"
    :settings="settings"
    :tiles="tiles"
    :start="index"
    @save="saveScreens"
    @close="editing = false"
  />
  <main v-else-if="current" :key="current.name" class="grid">
    <TileCard
      v-for="t in current.tiles"
      :key="t.tile_id"
      :tile-id="t.tile_id"
      :size="t.size"
      :tile="tiles[t.tile_id]"
      title="Double-click to expand"
      @dblclick="expanded = t.tile_id"
    />
    <p v-if="current.tiles.length === 0" class="notice">This screen has no running modules.</p>
  </main>
  <div v-if="expanded" class="overlay fullscreen" @click.self="expanded = null">
    <TileCard :tile-id="expanded" size="full" :tile="tiles[expanded]" @dblclick="expanded = null">
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
