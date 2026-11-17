/**
 * Sizing for the display widget, kept apart from DisplayText.vue so
 * `pnpm test` can run it in Node.
 */

/** The smallest and largest font size, in px, the text is scaled to. */
export const MIN_SIZE = 10
export const MAX_SIZE = 600

/**
 * The font size that makes text fill a box. `textW` and `textH` are the
 * text's measured size at `measuredAt` px; text scales linearly with its
 * font size. Returns `MIN_SIZE` for an empty or unmeasured box.
 */
export function fitFontSize(
  boxW: number,
  boxH: number,
  textW: number,
  textH: number,
  measuredAt: number,
): number {
  if (!(boxW > 0 && boxH > 0 && textW > 0 && textH > 0)) return MIN_SIZE
  const scale = Math.min(boxW / textW, boxH / textH)
  return Math.min(MAX_SIZE, Math.max(MIN_SIZE, Math.floor(measuredAt * scale)))
}
