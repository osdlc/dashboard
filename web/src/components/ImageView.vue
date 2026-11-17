<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import type { Widget } from '../contract'
import { apiHeaders } from '../session'

type Image = Extract<Widget, { type: 'image' }>
const props = defineProps<{ widget: Image }>()

/**
 * The picture is a file on the dashboard's machine. The server sends it from
 * /api/image, but only while a tile shows it, and only with the session token,
 * which can't go in an <img> URL: so it is fetched and shown as a blob.
 */
const src = ref<string | null>(null)
const failed = ref(false)
let loading = 0

async function load(path: string) {
  const mine = ++loading
  failed.value = false
  try {
    const res = await fetch(`/api/image?path=${encodeURIComponent(path)}`, { headers: apiHeaders() })
    if (!res.ok) throw new Error(String(res.status))
    const url = URL.createObjectURL(await res.blob())
    // A newer picture was asked for while this one loaded.
    if (mine !== loading) return URL.revokeObjectURL(url)
    if (src.value) URL.revokeObjectURL(src.value)
    src.value = url
  } catch {
    if (mine === loading) failed.value = true
  }
}

watch(() => props.widget.path, load, { immediate: true })
onBeforeUnmount(() => {
  loading++
  if (src.value) URL.revokeObjectURL(src.value)
})
</script>

<template>
  <figure class="w-image">
    <div class="frame" :class="{ empty: !src }">
      <img v-if="src" :src="src" :alt="widget.caption ?? ''" />
      <span v-else-if="failed">Can't load this picture here</span>
    </div>
    <figcaption v-if="widget.caption" :title="widget.path">{{ widget.caption }}</figcaption>
  </figure>
</template>
