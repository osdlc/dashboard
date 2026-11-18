<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import type { ClientMessage, UiSettings } from '../contract'

const props = defineProps<{ settings: UiSettings; copy: boolean; before: number }>()
const emit = defineEmits<{ close: []; created: [blank: boolean]; send: [msg: ClientMessage] }>()

const name = ref('')
const copy = ref(props.copy)
const waiting = ref(false)
const input = ref<HTMLInputElement>()
onMounted(() => input.value?.focus())

/** This dialog's request: outcomes are counted, and the last one stays in the settings. */
const outcome = computed(() => {
  const created = props.settings.config.created
  return created && (created.seq ?? 0) > props.before ? created : null
})
watch(outcome, (o) => {
  if (!o) return
  waiting.value = false
  if (!o.error) emit('created', !copy.value)
})

function submit() {
  if (waiting.value || !name.value.trim()) return
  waiting.value = true
  emit('send', { type: 'create_dashboard', name: name.value, copy: copy.value })
}
</script>

<template>
  <div class="overlay" @click.self="emit('close')" @keydown.esc="emit('close')">
    <section class="help new-dashboard" role="dialog" aria-label="New dashboard">
      <header>
        <h2>{{ copy ? 'Save dashboard as' : 'New dashboard' }}</h2>
        <button type="button" @click="emit('close')">Close</button>
      </header>
      <form @submit.prevent="submit">
        <label>
          Name
          <input ref="input" v-model="name" type="text" autocomplete="off" spellcheck="false" />
        </label>
        <div class="choices">
          <label><input v-model="copy" type="radio" :value="false" /> Blank</label>
          <label><input v-model="copy" type="radio" :value="true" /> Copy of current</label>
        </div>
        <p class="muted">
          {{
            copy
              ? `A copy of ${settings.config.dashboard ?? 'config.yaml'}, as its own file you can change.`
              : 'One empty screen. Modules in config.yaml keep running; add what you want to show.'
          }}
        </p>
        <p v-if="outcome?.error" class="error" role="alert">{{ outcome.error }}</p>
        <button type="submit" :disabled="waiting || !name.trim()">Create</button>
      </form>
    </section>
  </div>
</template>
