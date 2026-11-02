// The widget registry: every catalog entry, from src/data/registry.json, which
// scripts/registry.mjs writes from `dashboard widgets --json`.
import data from '../data/registry.json'
import shotSizes from '../data/registry-shots.json'
import type { Widget } from './config-example.ts'
import type { TileCapture } from './term-tile.ts'

export { configExample, settings, type Setting, type Widget } from './config-example.ts'

export const widgets: Widget[] = (data.entries as unknown as Widget[])
  .slice()
  .sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id))

/** Tags with how many widgets carry each, most used first. */
export function tagCounts(): [string, number][] {
  const counts = new Map<string, number>()
  for (const w of widgets) for (const t of w.tags) counts.set(t, (counts.get(t) ?? 0) + 1)
  return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
}

/** Built in, bundled into the binary, or a plugin package. */
export function kindLabel(w: Widget): string {
  if (w.kind === 'builtin') return 'built-in'
  return w.source === 'bundled' ? 'bundled plugin' : 'plugin package'
}

/** Built from an API's docs and a sample response, never run against the real service. */
export const untestedLive = (w: Widget) =>
  w.tags.includes('untested-live') || /untested live/i.test(w.description)

/** Tile sizes, as the contract spells them, smallest first. */
export const SIZES = ['small', 'medium', 'large', 'wide', 'tall'] as const
export type Size = (typeof SIZES)[number]

/** A web picture of a demo view, under public/, and its size in CSS pixels. */
export interface Shot {
  path: string
  width: number
  height: number
}

/** A sizes file (scripts/registry.mjs): the themes, and each picture's size by ID and tile size. */
export type ShotSizes = { themes: string[]; shots: Record<string, Partial<Record<Size, [number, number]>>> }

/** Looks up pictures in `dir` (under public/) from its sizes file. */
export function pictures(sizes: ShotSizes, dir: string) {
  return (id: string, theme: string, size: Size): Shot | null => {
    const dims = sizes.shots[id]?.[size]
    return dims ? { path: `${dir}/${theme}/${id}-${size}.webp`, width: dims[0], height: dims[1] } : null
  }
}

/** Terminal captures by theme for one ID, from an import.meta.glob of `<dir>/<theme>/<id>.json`. */
export function captures(files: Record<string, TileCapture>, id: string): { theme: string; capture: TileCapture }[] {
  return Object.entries(files)
    .map(([path, capture]) => ({ capture, m: /\/([^/]+)\/([^/]+)\.json$/.exec(path) }))
    .filter(({ m }) => m?.[2] === id)
    .map(({ capture, m }) => ({ theme: m![1], capture }))
}

const shots = shotSizes as ShotSizes
const widgetShot = pictures(shots, 'registry')

/** Themes the web pictures were taken in (scripts/registry.mjs, THEMES). */
export const shotThemes: string[] = shots.themes

/** The demo view's web picture at `size` (default: the widget's own) in `theme`. */
export function shot(w: Widget, theme: string, size: Size = w.default_size): Shot | null {
  return widgetShot(w.id, theme, size)
}

const tuiFiles = import.meta.glob<TileCapture>('../data/tui-tiles/*/*.json', { eager: true, import: 'default' })

/** The demo view drawn by the terminal UI at every size, in each theme it was captured in. */
export function tuiTiles(w: Widget): { theme: string; capture: TileCapture }[] {
  return captures(tuiFiles, w.id)
}
