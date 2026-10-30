import { ref, watchEffect } from 'vue'

/** Spacing between and inside tiles. Remembered per browser; this is a viewer preference, not config. */
export type Density = 'comfortable' | 'compact'
const KEY = 'dashboard.density'

function load(): Density {
  try {
    return localStorage.getItem(KEY) === 'compact' ? 'compact' : 'comfortable'
  } catch {
    return 'comfortable'
  }
}

export function useDensity() {
  const density = ref<Density>(load())
  watchEffect(() => {
    document.documentElement.dataset.density = density.value
    try {
      localStorage.setItem(KEY, density.value)
    } catch {
      // Storage blocked (private window): the choice just isn't remembered.
    }
  })
  const toggle = () => (density.value = density.value === 'compact' ? 'comfortable' : 'compact')
  return { density, toggle }
}
