/** True when the OS asks for less motion; animations are skipped then. */
export function reducedMotion(): boolean {
  return window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false
}

/**
 * Keys that stay the same for the same item across updates, so list
 * transitions can tell new rows from moved ones. Repeats get "#2", "#3"….
 */
export function stableKeys<T>(items: readonly T[], key: (item: T) => string): string[] {
  const seen = new Map<string, number>()
  return items.map((item) => {
    const k = key(item)
    const n = (seen.get(k) ?? 0) + 1
    seen.set(k, n)
    return n === 1 ? k : `${k}#${n}`
  })
}
