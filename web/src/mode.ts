/**
 * `?mode=ambient`: a view to look at, not use. No header or editor, large type,
 * screens rotating, nothing interactive. The desktop app's wallpaper and
 * screensaver windows load the page this way.
 */
export const ambient = new URLSearchParams(location.search).get('mode') === 'ambient'

/** How long each screen shows in ambient mode when the config sets no rotation. */
export const AMBIENT_INTERVAL_MS = 30_000
