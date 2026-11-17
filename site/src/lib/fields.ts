// A widget type's fields, from the view's JSON Schema (src/data/view-schema.json,
// `dashboard schema view`), for the building block pages. Also the widget types
// a view uses. Kept apart from blocks.ts, which loads the data, so tests can run
// it without the site (`pnpm test`).

type Schema = Record<string, any>

export interface Field {
  /** `label`, or `items[].title` for a field of each item in a list. */
  key: string
  type: string
  required: boolean
  /** The default as JSON, or '' when there is none. */
  default: string
  description: string
  /** A field of the items or object above it. */
  nested: boolean
}

/** Every widget type the schema knows, in its order (text, stat, gauge, ...). */
export function widgetTypes(schema: Schema): string[] {
  return (schema.$defs.Widget.oneOf as Schema[]).map((v) => v.properties.type.const)
}

/** The widget type's description, from its doc comment. */
export function typeDescription(schema: Schema, type: string): string {
  return variant(schema, type).description ?? ''
}

/** The fields of a widget type: required ones first, then the rest, each followed by the fields of its items. */
export function fields(schema: Schema, type: string): Field[] {
  const v = variant(schema, type)
  const out: Field[] = []
  for (const [key, raw, required] of ordered(v)) {
    if (key === 'type') continue
    out.push(field(schema, key, raw, required, false))
    // One level down: list items (ListItem, Slice, ...) and objects (Thresholds).
    const inner = innerObject(schema, raw)
    if (inner) {
      const prefix = isArray(raw) ? `${key}[].` : `${key}.`
      for (const [k, r, req] of ordered(inner)) out.push(field(schema, prefix + k, r, req, true))
    }
  }
  return out
}

/** Every widget type in a widget, its own first, then its children's and table cells'. */
export function typesIn(widget: Schema, out: string[] = []): string[] {
  if (!widget || typeof widget !== 'object' || typeof widget.type !== 'string') return out
  if (!out.includes(widget.type)) out.push(widget.type)
  for (const child of widget.children ?? []) typesIn(child, out)
  for (const row of widget.rows ?? []) for (const cell of Object.values(row.cells ?? {})) typesIn(cell as Schema, out)
  return out
}

function variant(schema: Schema, type: string): Schema {
  const v = (schema.$defs.Widget.oneOf as Schema[]).find((x) => x.properties.type.const === type)
  if (!v) throw new Error(`no widget type ${type} in the schema`)
  return v
}

function ordered(obj: Schema): [string, Schema, boolean][] {
  const required = new Set<string>(obj.required ?? [])
  const all = Object.entries((obj.properties ?? {}) as Record<string, Schema>).map(
    ([k, v]) => [k, v, required.has(k)] as [string, Schema, boolean],
  )
  return [...all.filter((f) => f[2]), ...all.filter((f) => !f[2])]
}

function field(schema: Schema, key: string, raw: Schema, required: boolean, nested: boolean): Field {
  const target = deref(schema, raw)
  const d = raw.default
  return {
    key,
    type: typeText(schema, raw),
    required,
    default: d === undefined || d === null || (Array.isArray(d) && !d.length) ? '' : JSON.stringify(d),
    description: raw.description ?? target?.description ?? '',
    nested,
  }
}

/** The `$ref` a property points at, through `anyOf: [ref, null]`, or null. */
function refName(raw: Schema): string | null {
  const ref: string | undefined = raw.$ref ?? raw.anyOf?.find((x: Schema) => x.$ref)?.$ref
  return ref?.startsWith('#/$defs/') ? ref.slice('#/$defs/'.length) : null
}

function deref(schema: Schema, raw: Schema): Schema | null {
  const name = refName(raw)
  return name ? schema.$defs[name] : null
}

function isArray(raw: Schema): boolean {
  return raw.type === 'array' || (Array.isArray(raw.type) && raw.type.includes('array'))
}

/** The object each item of a list is, or the object a field is, when it has fields of its own. */
function innerObject(schema: Schema, raw: Schema): Schema | null {
  const target = isArray(raw) ? deref(schema, raw.items ?? {}) : deref(schema, raw)
  return target?.properties && refName(isArray(raw) ? raw.items : raw) !== 'Widget' ? target : null
}

/** Enum values of a definition, from `enum` or `oneOf` of consts. */
function values(def: Schema): string[] | null {
  if (Array.isArray(def.enum)) return def.enum
  if (!Array.isArray(def.oneOf)) return null
  const each = def.oneOf.map((x: Schema) => ('const' in x ? [x.const] : x.enum))
  return each.every(Array.isArray) ? each.flat() : null
}

const NAMES: Record<string, string> = {
  Widget: 'widget',
  Tone: 'tone',
  Action: 'action',
  Cell: 'text, number or widget',
}

function typeText(schema: Schema, raw: Schema): string {
  const name = refName(raw)
  if (name) {
    if (NAMES[name]) return NAMES[name]
    const def = schema.$defs[name]
    const vs = values(def)
    if (vs) return vs.length <= 5 ? vs.join(' | ') : name.toLowerCase()
    return 'object'
  }
  if (isArray(raw)) {
    const items = raw.items ?? {}
    // Points: [unix ms, value] pairs.
    if (Array.isArray(items.prefixItems) || Array.isArray(items.items)) return '[ms, value] list'
    const inner = refName(items)
    if (inner === 'Widget') return 'widget list'
    // A list of an enum: `status list`, not every value.
    if (inner && !schema.$defs[inner].properties) return `${NAMES[inner] ?? inner.toLowerCase()} list`
    if (inner) return 'list'
    return `${typeText(schema, items)} list`
  }
  if (raw.type === 'object' && raw.additionalProperties) return `map of ${typeText(schema, raw.additionalProperties)}`
  const t = Array.isArray(raw.type) ? raw.type.filter((x: string) => x !== 'null') : [raw.type]
  return t.map((x: string) => (x === 'integer' ? 'integer' : x)).join(' | ') || 'any'
}

/** Text as HTML with `backticked` parts as <code>, for descriptions. */
export function inlineCode(text: string): string {
  const esc = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
  return esc.replace(/`([^`]+)`/g, '<code>$1</code>')
}
