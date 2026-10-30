import { computed, onMounted, onUnmounted, ref, type ComputedRef, type Ref } from 'vue'
import type { UiSettings } from './contract'

export type RotationState = 'on' | 'paused' | 'off' | 'none'

/**
 * Automatic screen rotation, matching the TUI (crates/tui/src/rotate.rs):
 * user activity pauses it until idle for `resume_after_ms`; `toggle` turns it off and on.
 */
export function useRotation(
  settings: Ref<UiSettings | null>,
  index: ComputedRef<number>,
  go: (i: number) => void,
) {
  const enabled = ref(true)
  const now = ref(Date.now())
  const lastInput = ref<number | null>(null)
  let lastSwitch = Date.now()

  const order = computed(() => {
    const s = settings.value
    if (!s?.rotate) return []
    return s.rotate.screens.map((name) => s.screens.findIndex((x) => x.name === name)).filter((i) => i >= 0)
  })

  const state = computed<RotationState>(() => {
    const r = settings.value?.rotate
    if (!r || order.value.length < 2) return 'none'
    if (!enabled.value) return 'off'
    if (lastInput.value !== null && now.value - lastInput.value < r.resume_after_ms) return 'paused'
    return 'on'
  })

  function input() {
    lastInput.value = Date.now()
    lastSwitch = lastInput.value
  }

  function toggle() {
    enabled.value = !enabled.value
    lastInput.value = null
    lastSwitch = Date.now()
  }

  function tick() {
    now.value = Date.now()
    const r = settings.value?.rotate
    if (!r || state.value !== 'on' || now.value - lastSwitch < r.interval_ms) return
    lastSwitch = now.value
    const pos = order.value.indexOf(index.value)
    const next = pos >= 0 ? order.value[(pos + 1) % order.value.length] : order.value[0]
    if (next !== undefined) go(next)
  }

  let timer: number | undefined
  let lastMove = 0
  const onActivity = (e: Event) => {
    // Pointer moves fire constantly; count at most one per second.
    if (e.type === 'pointermove') {
      if (Date.now() - lastMove < 1000) return
      lastMove = Date.now()
    }
    if (lastInput.value === null || Date.now() - lastInput.value > 250) input()
  }
  const events = ['pointerdown', 'pointermove', 'wheel'] as const
  onMounted(() => {
    timer = window.setInterval(tick, 1000)
    for (const e of events) window.addEventListener(e, onActivity, { passive: true })
  })
  onUnmounted(() => {
    window.clearInterval(timer)
    for (const e of events) window.removeEventListener(e, onActivity)
  })

  return { state, toggle, input }
}
