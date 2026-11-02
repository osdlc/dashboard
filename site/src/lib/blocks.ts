// The building blocks: one example view per widget type and a few that combine
// them, from src/data/blocks.json, which scripts/registry.mjs writes from
// examples/blocks/*.json. Drawn like the widgets' demo views.
import data from '../data/blocks.json'
import shotSizes from '../data/block-shots.json'
import viewSchema from '../data/view-schema.json'
import { fields, inlineCode, typeDescription, typesIn, widgetTypes, type Field } from './fields.ts'
import { captures, pictures, widgets, type ShotSizes, type Size, type Widget } from './registry.ts'
import type { TileCapture } from './term-tile.ts'

export { inlineCode, type Field }

export interface Block {
  id: string
  /** The block's title, e.g. "Series: area". */
  name: string
  description: string
  default_size: Size
  group: 'primitives' | 'compositions'
  /** examples/blocks/<id>.json, relative to the repository. */
  source: string
  demo: { status: string; summary: string; root: Record<string, any> }
}

const schema = viewSchema as Record<string, any>

/** Widget types in the contract's order: text, stat, gauge, series, ... */
export const TYPES = widgetTypes(schema)

/** The widget type at the block's root. */
export const typeOf = (b: Block): string => b.demo.root.type

const all = data.entries as unknown as Block[]
const byType = (a: Block, b: Block) => TYPES.indexOf(typeOf(a)) - TYPES.indexOf(typeOf(b)) || a.name.localeCompare(b.name)

/** One example per widget type, in the contract's order, then the ones that combine them. */
export const blocks: Block[] = [
  ...all.filter((b) => b.group === 'primitives').sort(byType),
  ...all.filter((b) => b.group === 'compositions').sort((a, b) => a.name.localeCompare(b.name)),
]

export const GROUPS = [
  {
    id: 'primitives' as const,
    title: 'Primitives',
    blurb: 'Every widget type a view can hold, each with its main options.',
  },
  {
    id: 'compositions' as const,
    title: 'Compositions',
    blurb: 'Rows, columns and tables put the primitives together into bigger tiles.',
  },
]

const shots = shotSizes as ShotSizes
const shotOf = pictures(shots, 'block-shots')

/** Themes the web pictures were taken in. */
export const blockShotThemes: string[] = shots.themes

/** The block's web picture at `size` (default: its own) in `theme`. */
export function blockShot(b: Block, theme: string, size: Size = b.default_size) {
  return shotOf(b.id, theme, size)
}

const tuiFiles = import.meta.glob<TileCapture>('../data/block-tiles/*/*.json', { eager: true, import: 'default' })

/** The block drawn by the terminal UI at every size, in each theme it was captured in. */
export function blockTiles(b: Block) {
  return captures(tuiFiles, b.id)
}

/** The fields of a widget type, from the view's JSON Schema. */
export const typeFields = (type: string): Field[] => fields(schema, type)

/** What a widget type is, from its doc comment in the contract. */
export const aboutType = (type: string): string => typeDescription(schema, type)

/** Every widget type the block uses, its root's first. */
export const typesOf = (b: Block): string[] => typesIn(b.demo.root)

/** Widgets in the registry whose demo view uses `type` anywhere. */
export function widgetsUsing(type: string): Widget[] {
  return widgets.filter((w) => typesIn((w.demo as { root?: Record<string, any> }).root ?? {}).includes(type))
}

/** The primitive block that shows `type` first, for links from compositions. */
export const blockFor = (type: string): Block | undefined =>
  blocks.find((b) => b.group === 'primitives' && typeOf(b) === type)

/** The view as JSON, with each point and other short pairs on one line. */
export function viewJson(b: Block): string {
  return JSON.stringify(b.demo, null, 2).replace(/\[\n\s+(-?[\d.e+]+),\n\s+(-?[\d.e+]+)\n\s+\]/g, '[$1, $2]')
}
