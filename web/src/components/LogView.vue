<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import type { Widget } from '../contract'
import { stableKeys } from '../motion'

type Log = Extract<Widget, { type: 'log' }>
const props = defineProps<{ widget: Log }>()

const keys = computed(() => stableKeys(props.widget.lines, (l) => `${l.time ?? ''}|${l.text}`))
const box = ref<HTMLElement | null>(null)
/** Follow new lines unless the reader has scrolled up. */
const following = ref(true)

function onScroll() {
  const el = box.value
  if (el) following.value = el.scrollHeight - el.scrollTop - el.clientHeight < 24
}

function toBottom() {
  const el = box.value
  if (el) el.scrollTop = el.scrollHeight
  following.value = true
}

watch(
  () => props.widget.lines,
  async () => {
    if (!following.value) return
    await nextTick()
    toBottom()
  },
  { immediate: true, flush: 'post' },
)
</script>

<template>
  <div class="w-log">
    <div v-if="widget.label" class="w-label">{{ widget.label }}</div>
    <div ref="box" class="log-lines" role="log" @scroll="onScroll">
      <TransitionGroup name="log">
      <div v-for="(l, i) in widget.lines" :key="keys[i]" class="log-line" :data-level="l.level ?? undefined">
        <span v-if="l.time" class="log-time">{{ l.time }}</span>
        <span v-if="l.level" class="log-level">{{ l.level }}</span>
        <span class="log-text">{{ l.text }}</span>
      </div>
      </TransitionGroup>
    </div>
    <button v-if="!following" type="button" class="log-follow" @click="toBottom">↓ Follow</button>
  </div>
</template>
