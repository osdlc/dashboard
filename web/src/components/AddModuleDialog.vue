<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import type { CatalogEntry, ClientMessage, Size, TileUpdate } from '../contract'
import { buildConfig, fields, freeId, installed } from '../catalog'
import { inlineMarkdown } from '../format'
import TileCard from './TileCard.vue'

const SIZES: Size[] = ['small', 'medium', 'large', 'wide', 'tall']

/**
 * `screen` is the screen the tile goes on; `taken` the module IDs already in
 * use; `busy` is set while an earlier add is still being written.
 */
const props = defineProps<{ entry: CatalogEntry; screen: string | null; taken: Set<string>; busy?: boolean }>()
const emit = defineEmits<{ add: [msg: Extract<ClientMessage, { type: 'add_module' }>]; close: [] }>()

const tileId = ref(freeId(props.entry.id, props.taken))
const size = ref<Size>(props.entry.default_size)
const demo = ref(false)
const form = computed(() => fields(props.entry.config_schema))
// Empty means: leave it out and use the module's default.
const values = ref<Record<string, string>>(Object.fromEntries(form.value.map((f) => [f.key, ''])))
const errors = ref<Record<string, string>>({})
const canAdd = computed(() => installed(props.entry))

const idError = computed(() => {
  const id = tileId.value.trim()
  if (!/^[A-Za-z][A-Za-z0-9_-]*$/.test(id)) return 'Letters, digits, - and _, starting with a letter.'
  if (props.taken.has(id)) return `${id} is already in the config.`
  return ''
})

/** The demo view, shaped like a live tile so the usual components draw it. */
const preview = computed<TileUpdate>(() => ({ tile_id: props.entry.id, view: props.entry.demo, stale: false, demo: true, updated_at: 0 }))
const req = computed(() => props.entry.requirements ?? {})

function submit() {
  const built = buildConfig(form.value, values.value)
  errors.value = built.errors
  if (idError.value || Object.keys(built.errors).length || !canAdd.value || props.busy) return
  emit('add', {
    type: 'add_module',
    module_id: tileId.value.trim(),
    entry: props.entry.id,
    kind: props.entry.kind,
    config: built.config,
    demo: demo.value,
    screen: props.screen,
    size: size.value,
  })
}

const first = ref<HTMLInputElement | null>(null)
onMounted(() => first.value?.focus())
</script>

<template>
  <div class="overlay add-overlay" @click.self="emit('close')">
    <form class="add-dialog" role="dialog" aria-modal="true" :aria-label="`Add ${entry.name}`" @submit.prevent="submit">
      <header class="add-head">
        <div>
          <h2>{{ entry.name }}</h2>
          <p class="muted">
            {{ entry.id }} · {{ entry.source === 'bundled' ? 'bundled plugin' : entry.kind === 'builtin' ? 'built-in' : installed(entry) ? 'plugin package' : 'marketplace' }}
            · v{{ entry.version }}
          </p>
        </div>
        <button type="button" class="close" aria-label="Close" title="Close (Esc)" @click="emit('close')">✕</button>
      </header>
      <p class="add-desc">{{ entry.description }}</p>
      <p v-if="entry.tags?.length" class="add-tags">
        <span v-for="t in entry.tags" :key="t" class="tag">{{ t }}</span>
      </p>

      <div class="add-cols">
        <section class="add-preview" aria-label="Demo preview">
          <h3>Preview <small class="muted">made-up data</small></h3>
          <!-- Made-up data: nothing in it can be focused or clicked. -->
          <div class="grid preview-grid" inert>
            <TileCard :tile-id="tileId || entry.id" :size="size" :tile="preview" />
          </div>
          <dl v-if="req.binaries?.length || req.env?.length || req.hosts?.length" class="add-needs">
            <template v-if="req.binaries?.length"><dt>Programs</dt><dd>{{ req.binaries.join(', ') }}</dd></template>
            <template v-if="req.env?.length"><dt>Environment</dt><dd>{{ req.env.join(', ') }}</dd></template>
            <template v-if="req.hosts?.length"><dt>Calls</dt><dd>{{ req.hosts.join(', ') }}</dd></template>
          </dl>
        </section>

        <section class="add-form" aria-label="Settings">
          <h3>Settings</h3>
          <label class="field">
            <span>Module ID</span>
            <input ref="first" v-model="tileId" type="text" spellcheck="false" autocomplete="off" />
            <small v-if="idError" class="field-error">{{ idError }}</small>
            <small v-else class="muted">The key under <code>modules:</code>.</small>
          </label>
          <label class="field">
            <span>Size</span>
            <select v-model="size">
              <option v-for="s in SIZES" :key="s" :value="s">{{ s }}{{ s === entry.default_size ? ' (suggested)' : '' }}</option>
            </select>
          </label>
          <label class="field check">
            <input v-model="demo" type="checkbox" />
            <span>Demo data only <small class="muted">(nothing runs; no token needed)</small></span>
          </label>

          <p v-if="form.length === 0" class="muted">No settings.</p>
          <label v-for="f in form" :key="f.key" class="field" :data-kind="f.kind">
            <span>{{ f.key }}<b v-if="f.required" title="Required"> *</b></span>
            <select v-if="f.kind === 'choice' || f.kind === 'boolean'" v-model="values[f.key]">
              <option value="">default{{ f.placeholder ? ` (${f.placeholder})` : '' }}</option>
              <template v-if="f.kind === 'boolean'">
                <option value="true">true</option>
                <option value="false">false</option>
              </template>
              <option v-for="c in f.choices" v-else :key="c.value" :value="c.value">{{ c.label }}</option>
            </select>
            <textarea
              v-else-if="f.kind === 'lines' || f.kind === 'json'"
              v-model="values[f.key]"
              rows="3"
              spellcheck="false"
              :placeholder="f.placeholder"
            />
            <input
              v-else
              v-model="values[f.key]"
              :type="f.kind === 'text' ? 'text' : 'number'"
              :step="f.kind === 'integer' ? 1 : 'any'"
              spellcheck="false"
              :placeholder="f.placeholder"
            />
            <small v-if="errors[f.key]" class="field-error">{{ errors[f.key] }}</small>
            <small v-if="f.description" class="muted">
              <template v-for="(part, i) in inlineMarkdown(f.description)" :key="i">
                <code v-if="part.kind === 'code'">{{ part.text }}</code>
                <template v-else>{{ part.text }}</template>
              </template>
              <template v-if="f.kind === 'lines'"> One per line.</template>
              <template v-else-if="f.kind === 'json'"> As JSON.</template>
            </small>
          </label>
        </section>
      </div>

      <footer class="add-foot">
        <span v-if="!canAdd" class="muted">
          Not installed. Install it first: <code>dashboard plugin install {{ entry.id }}</code>
        </span>
        <span v-else class="muted">
          Writes <code>modules.{{ tileId.trim() }}</code>{{ screen ? ` and a tile on “${screen}”` : '' }} to the config file; comments stay.
        </span>
        <span class="spacer" />
        <button type="button" @click="emit('close')">Cancel</button>
        <button type="submit" class="primary" :disabled="!canAdd || !!idError || busy">Add{{ screen ? ` to ${screen}` : '' }}</button>
      </footer>
    </form>
  </div>
</template>
