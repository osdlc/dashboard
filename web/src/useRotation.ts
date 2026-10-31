import { computed, onMounted, onUnmounted, ref, type ComputedRef, type Ref } from 'vue'
import type { UiSettings } from './contract'
import { AMBIENT_INTERVAL_MS } from './mode'

export type RotationState = 'on' | 'paused' | 'off' | 'none'

/**
 * Automatic screen rotation, matching the TUI (crates/tui/src/rotate.rs):
 * user activity pauses it until idle for `resume_after_ms`; `toggle` turns it off and on.
 *
 * With `ambient`, every screen rotates even when the config sets no rotation
 * (every `AMBIENT_INTERVAL_MS`), and activity doesn't pause it.
 */
export function useRotation(
  settings: Ref<UiSettings | null>,
  index: ComputedRef<number>,
  go: (i: number) => void,
  ambient = false,
) {
  const enabled = ref(true)
  const now = ref(Date.now())
  const lastInput = ref<number | null>(null)
  let lastSwitch = Date.now()

  const order = computed(() => {
    const s = settings.value
    if (ambient && s && !s.rotate) return s.screens.map((_, i) => i)
    if (!s?.rotate) return []
    return s.rotate.screens.map((name) => s.screens.findIndex((x) => x.name === name)).filter((i) => i >= 0)
  })

  const interval = () => settings.value?.rotate?.interval_ms ?? (ambient ? AMBIENT_INTERVAL_MS : null)

  const state = computed<RotationState>(() => {
    const r = settings.value?.rotate
    if (interval() === null || order.value.length < 2) return 'none'
    if (ambient) return 'on'
    if (!enabled.value) return 'off'
    if (r && lastInput.value !== null && now.value - lastInput.value < r.resume_after_ms) return 'paused'
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
    const ms = interval()
    if (ms === null || state.value !== 'on' || now.value - lastSwitch < ms) return
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
    if (!ambient) for (const e of events) window.addEventListener(e, onActivity, { passive: true })
  })
  onUnmounted(() => {
    window.clearInterval(timer)
    for (const e of events) window.removeEventListener(e, onActivity)
  })

  return { state, toggle, input }
}
