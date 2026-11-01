<script setup lang="ts">
import type { ClientMessage, UiSettings } from '../contract'
import { formatMs } from '../format'

defineProps<{ settings: UiSettings }>()
const emit = defineEmits<{ close: []; send: [msg: ClientMessage] }>()

const themes = ['auto', 'light', 'dark'] as const
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <section class="help" role="dialog" aria-label="Settings">
      <header>
        <h2>Settings</h2>
        <button type="button" @click="emit('close')">Close</button>
      </header>

      <h3>Config file</h3>
      <p class="path"><code>{{ settings.config.path }}</code></p>
      <p v-if="!settings.config.exists" class="muted">Not created yet; showing the built-in demo.</p>
      <div v-if="settings.config.error" class="config-error" role="alert">
        <strong>Not applied</strong> (previous config still in effect)
        <pre>{{ settings.config.error }}</pre>
      </div>
      <p class="muted">Edit the file directly for screens and modules; changes apply while running.</p>
      <template v-if="settings.config.layers?.length">
        <h3>Config layers</h3>
        <p class="muted">Merged in this order; later files win. Settings are saved to the dashboard file, else config.yaml.</p>
        <ul class="layers">
          <li v-for="l in settings.config.layers" :key="l"><code>{{ l }}</code></li>
        </ul>
      </template>

      <h3>Theme</h3>
      <div class="theme-choice" role="radiogroup" aria-label="Theme">
        <button
          v-for="t in themes"
          :key="t"
          type="button"
          role="radio"
          :aria-checked="settings.theme === t"
          @click="emit('send', { type: 'set_theme', theme: t, etag: settings.config.etag ?? null })"
        >
          {{ t }}
        </button>
      </div>
      <p class="muted">Saved to the config file; your comments and formatting are kept.</p>

      <h3>Rotation</h3>
      <p v-if="settings.rotate">
        Every {{ formatMs(settings.rotate.interval_ms) }} through {{ settings.rotate.screens.join(', ') }}; resumes
        {{ formatMs(settings.rotate.resume_after_ms) }} after input.
      </p>
      <p v-else class="muted">Not configured. Add <code>rotate:</code> to the config.</p>

      <h3>Modules</h3>
      <table class="w-table">
        <thead>
          <tr><th>Module</th><th>Refresh</th><th>Source</th></tr>
        </thead>
        <tbody>
          <tr v-for="m in settings.modules" :key="m.tile_id">
            <td>{{ m.name }} <span class="muted">({{ m.tile_id }})</span></td>
            <td>{{ m.refresh_ms != null ? formatMs(m.refresh_ms) : 'push' }}</td>
            <td class="muted">{{ m.refresh_overridden ? 'config' : 'module default' }}</td>
          </tr>
        </tbody>
      </table>
    </section>
  </div>
</template>
