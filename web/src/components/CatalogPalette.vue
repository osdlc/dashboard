<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import type { CatalogEntry, ModuleSummary } from '../contract'
import { fetchCatalog, installed } from '../catalog'

/**
 * The editor's "add" list: running modules (added straight to the screen)
 * and the catalog of everything known (opens the add dialog).
 */
const props = defineProps<{ modules: ModuleSummary[]; onScreen: Set<string> }>()
const emit = defineEmits<{ addTile: [tileId: string]; pick: [entry: CatalogEntry] }>()

const query = ref('')
const tag = ref('')
const entries = ref<CatalogEntry[]>([])
const tags = ref<string[]>([])
const problems = ref<string[]>([])
const loading = ref(false)
const failed = ref('')

let controller: AbortController | null = null
let timer: number | undefined
async function load() {
  controller?.abort()
  const mine = new AbortController()
  controller = mine
  loading.value = true
  try {
    const r = await fetchCatalog(query.value, tag.value, mine.signal)
    entries.value = r.entries
    tags.value = r.tags
    problems.value = r.errors ?? []
    failed.value = ''
  } catch (e) {
    if ((e as Error).name !== 'AbortError') failed.value = String(e)
  } finally {
    // A newer request is still loading when this one was cut short.
    if (controller === mine) loading.value = false
  }
}
// Search on the host as you type, a moment after the last key.
watch(query, () => {
  window.clearTimeout(timer)
  timer = window.setTimeout(load, 150)
})
watch(tag, load)
load()
onUnmounted(() => {
  window.clearTimeout(timer)
  controller?.abort()
})

const running = computed(() => {
  const words = query.value.toLowerCase().split(/\s+/).filter(Boolean)
  return [...props.modules]
    .filter((m) => words.every((w) => `${m.tile_id} ${m.name}`.toLowerCase().includes(w)))
    .sort((a, b) => a.tile_id.localeCompare(b.tile_id))
})
const runningIds = computed(() => new Set(props.modules.map((m) => m.tile_id)))
</script>

<template>
  <aside class="palette catalog" aria-label="Add a component">
    <h3>Add component</h3>
    <input v-model="query" type="search" class="catalog-search" placeholder="Search widgets…" aria-label="Search widgets" spellcheck="false" />
    <div class="catalog-tags" role="group" aria-label="Filter by tag">
      <button type="button" :aria-pressed="tag === ''" @click="tag = ''">all</button>
      <button v-for="t in tags" :key="t" type="button" :aria-pressed="tag === t" @click="tag = tag === t ? '' : t">{{ t }}</button>
    </div>

    <template v-if="running.length && !tag">
      <h4>Running</h4>
      <button
        v-for="m in running"
        :key="m.tile_id"
        type="button"
        class="palette-item"
        :data-used="onScreen.has(m.tile_id) || undefined"
        :title="`Add ${m.tile_id} to this screen (${m.default_size})`"
        @click="emit('addTile', m.tile_id)"
      >
        <span>{{ m.tile_id }}</span>
        <small>{{ m.name }} · {{ m.default_size }}</small>
      </button>
    </template>

    <h4>Catalog <small v-if="loading" class="muted">…</small></h4>
    <p v-if="failed" class="field-error">{{ failed }}</p>
    <p v-else-if="!loading && entries.length === 0" class="muted">Nothing matches.</p>
    <button
      v-for="e in entries"
      :key="`${e.kind}:${e.id}`"
      type="button"
      class="palette-item catalog-item"
      :data-running="runningIds.has(e.id) || undefined"
      :data-market="!installed(e) || undefined"
      :title="e.description"
      @click="emit('pick', e)"
    >
      <span>{{ e.name }} <small class="kind">{{ installed(e) ? (e.kind === 'builtin' ? '' : 'plugin') : 'marketplace' }}</small></span>
      <small>{{ e.id }} · {{ e.description }}</small>
    </button>
    <details v-if="problems.length" class="catalog-problems">
      <summary>{{ problems.length }} problem{{ problems.length === 1 ? '' : 's' }} reading packages</summary>
      <p v-for="p in problems" :key="p" class="muted">{{ p }}</p>
    </details>
  </aside>
</template>
