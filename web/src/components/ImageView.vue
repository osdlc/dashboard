<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { Widget } from '../contract'

type Image = Extract<Widget, { type: 'image' }>
const props = defineProps<{ widget: Image }>()

/**
 * The picture is a file on the dashboard's machine. The server sends it from
 * /api/image, with the page's session token, and only while a tile shows it.
 */
const token = new URLSearchParams(location.search).get('token') ?? ''
const src = computed(
  () => `/api/image?path=${encodeURIComponent(props.widget.path)}&token=${encodeURIComponent(token)}`,
)
const failed = ref(false)
watch(src, () => (failed.value = false))
</script>

<template>
  <figure class="w-image">
    <div class="frame" :class="{ empty: failed }">
      <img v-if="!failed" :src="src" :alt="widget.caption ?? ''" @error="failed = true" />
      <span v-else>Can't load this picture here</span>
    </div>
    <figcaption v-if="widget.caption" :title="widget.path">{{ widget.caption }}</figcaption>
  </figure>
</template>
