import { computed, onMounted, onUnmounted, ref, type Ref } from 'vue'
import type { UiSettings } from './contract'

/** Current screen, kept in the URL hash (`#screen=<name>`) so reloads keep it. */
export function useScreens(settings: Ref<UiSettings | null>) {
  const wanted = ref(readHash())
  const screens = computed(() => settings.value?.screens ?? [])
  const index = computed(() => Math.max(0, screens.value.findIndex((s) => s.name === wanted.value)))
  const current = computed(() => screens.value[index.value])

  function go(i: number) {
    const n = screens.value.length
    if (n === 0) return
    const screen = screens.value[((i % n) + n) % n]
    if (!screen) return
    wanted.value = screen.name
    history.replaceState(null, '', `${location.search}#screen=${encodeURIComponent(screen.name)}`)
  }

  const onHash = () => (wanted.value = readHash())
  onMounted(() => window.addEventListener('hashchange', onHash))
  onUnmounted(() => window.removeEventListener('hashchange', onHash))

  return { screens, index, current, go }
}

function readHash(): string {
  return decodeURIComponent(new URLSearchParams(location.hash.slice(1)).get('screen') ?? '')
}
