/**
 * `?mode=ambient`: a view to look at, not use. No header or editor, large type,
 * screens rotating, nothing interactive. The desktop app's wallpaper and
 * screensaver windows load the page this way.
 */
export const ambient = new URLSearchParams(location.search).get('mode') === 'ambient'

/** How long each screen shows in ambient mode when the config sets no rotation. */
export const AMBIENT_INTERVAL_MS = 30_000

/**
 * `?mode=demo&catalog=<url>`: catalog entries' demo views, with no server.
 * `catalog` is a `dashboard widgets --json` file; `id` picks one entry,
 * `theme` (`light` or `dark`) fixes the mode and `color_theme` (a built-in
 * name, e.g. `paper`) the colors, `size` draws the one tile at that size
 * instead of its default, and `fill` makes it fill the page. The site's
 * registry screenshots are taken from this page, and its widget pages embed
 * it (site/scripts/registry.mjs).
 */
export const demo = new URLSearchParams(location.search).get('mode') === 'demo'
