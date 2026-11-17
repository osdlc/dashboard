/**
 * Geometry for the clock_face widget, kept apart from ClockFace.vue so
 * `pnpm test` can run it in Node. The SVG's viewBox is -50 -50 100 100, so
 * the center is the origin and a hand is a line drawn straight up, turned by
 * its angle.
 */

/** Hour hand angle in degrees, clockwise from twelve: it moves with the minutes. */
export function hourAngle(hour: number, minute: number): number {
  return (hour % 12) * 30 + minute * 0.5
}

/** Minute hand angle in degrees; the seconds are left out, so it ticks. */
export function minuteAngle(minute: number): number {
  return minute * 6
}

/** Second hand angle in degrees. */
export function secondAngle(second: number): number {
  return second * 6
}

/**
 * The angle to turn a hand to so it only ever moves forward: from `prev`
 * (which may already be past 360) to the next time `target` comes round.
 * Going from 354 to 0 degrees gives 360, not a spin backwards, and a CSS
 * rotate transition then takes the short way.
 */
export function forward(prev: number | undefined, target: number): number {
  if (prev === undefined) return target
  const delta = (((target - prev) % 360) + 360) % 360
  return prev + delta
}

/** Hour tick marks: the 12 positions, with quarters (12, 3, 6, 9) heavier. */
export function ticks(): Array<{ angle: number; quarter: boolean }> {
  return Array.from({ length: 12 }, (_, i) => ({ angle: i * 30, quarter: i % 3 === 0 }))
}

/** "14:05" or "14:05:09", for the accessible name. */
export function timeText(hour: number, minute: number, second?: number | null): string {
  const two = (n: number) => String(n).padStart(2, '0')
  return `${two(hour)}:${two(minute)}` + (second == null ? '' : `:${two(second)}`)
}
