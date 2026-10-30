<script setup lang="ts">
import type { UiSettings } from '../contract'
import { formatMs } from '../format'
import { KEYS } from '../keys'

defineProps<{ settings: UiSettings; current: number }>()
const emit = defineEmits<{ close: [] }>()
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <section class="help" role="dialog" aria-label="Help">
      <header>
        <h2>Help</h2>
        <button type="button" @click="emit('close')">Close</button>
      </header>

      <h3>Keys</h3>
      <dl class="keys">
        <template v-for="[key, what] in KEYS" :key="key">
          <dt><kbd>{{ key }}</kbd></dt>
          <dd>{{ what }}</dd>
        </template>
      </dl>

      <h3>Screens</h3>
      <ol>
        <li v-for="(s, i) in settings.screens" :key="s.name" :class="{ current: i === current }">
          {{ s.name }} <span class="muted">({{ s.tiles.length }} tiles)</span>
        </li>
      </ol>

      <h3>Modules</h3>
      <ul>
        <li v-for="m in settings.modules" :key="m.tile_id">
          {{ m.name }}
          <span class="muted">({{ m.tile_id }}) every {{ m.refresh_ms != null ? formatMs(m.refresh_ms) : 'push' }}</span>
          <div v-if="m.commands?.length" class="muted">commands: {{ m.commands.join(', ') }}</div>
        </li>
      </ul>
    </section>
  </div>
</template>
