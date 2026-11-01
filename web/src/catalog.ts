import type { CatalogEntry, CatalogResponse } from './contract'
import { apiHeaders } from './session'

/** `GET /api/catalog`: entries matching `q` and `tag`, and every tag. */
export async function fetchCatalog(q: string, tag: string, signal?: AbortSignal): Promise<CatalogResponse> {
  const params = new URLSearchParams({ q, tag })
  const res = await fetch(`/api/catalog?${params}`, { headers: apiHeaders(), signal: signal ?? null })
  if (!res.ok) throw new Error(`catalog: HTTP ${res.status}`)
  return (await res.json()) as CatalogResponse
}

/** False for a marketplace entry that isn't installed yet. */
export const installed = (e: CatalogEntry) => e.source !== 'marketplace'

// ---- settings forms from a JSON Schema ----

type Schema = Record<string, unknown>

/** How a setting is edited. `json` is the fallback for anything else. */
export type FieldKind = 'text' | 'number' | 'integer' | 'boolean' | 'choice' | 'lines' | 'json'

export interface Field {
  key: string
  kind: FieldKind
  required: boolean
  description: string
  /** The default, as shown in an empty input. */
  placeholder: string
  /** For `choice`: the allowed values and their descriptions. */
  choices: { value: string; label: string }[]
}

/** One field per top-level property of a module's config schema. */
export function fields(schema: Schema | undefined | null): Field[] {
  if (!schema) return []
  const props = (schema.properties ?? {}) as Record<string, Schema>
  const required = new Set((schema.required as string[] | undefined) ?? [])
  return Object.entries(props).map(([key, raw]) => {
    const prop = resolve(schema, raw)
    const choices = choicesOf(schema, prop)
    const type = mainType(prop)
    let kind: FieldKind = 'json'
    if (choices.length) kind = 'choice'
    else if (type === 'string') kind = 'text'
    else if (type === 'integer' || type === 'number' || type === 'boolean') kind = type
    else if (type === 'array' && mainType(resolve(schema, (prop.items ?? {}) as Schema)) === 'string') kind = 'lines'
    const description = String(raw.description ?? prop.description ?? '')
    return { key, kind, required: required.has(key), description, placeholder: placeholder(raw.default ?? prop.default, kind), choices }
  })
}

/** Follow a local `$ref`, and unwrap `anyOf: [X, null]` (an optional setting). */
function resolve(root: Schema, s: Schema): Schema {
  const ref = s.$ref
  if (typeof ref === 'string' && ref.startsWith('#/$defs/')) {
    const defs = (root.$defs ?? {}) as Record<string, Schema>
    return resolve(root, defs[ref.slice('#/$defs/'.length)] ?? {})
  }
  const any = s.anyOf as Schema[] | undefined
  if (any) {
    const rest = any.filter((x) => x.type !== 'null')
    if (rest.length === 1 && rest[0]) return { ...resolve(root, rest[0]), description: s.description }
  }
  return s
}

/** The type that isn't `null`, e.g. `string` for `["string", "null"]`. */
function mainType(s: Schema): string | undefined {
  const t = s.type
  if (Array.isArray(t)) return t.find((x) => x !== 'null') as string | undefined
  return t as string | undefined
}

/** Allowed values from `enum`, or `oneOf` items that are each a `const` or an `enum`. */
function choicesOf(root: Schema, s: Schema): Field['choices'] {
  if (Array.isArray(s.enum)) return s.enum.filter((v) => v !== null).map((v) => ({ value: String(v), label: String(v) }))
  const one = s.oneOf as Schema[] | undefined
  if (!one) return []
  const out: Field['choices'] = []
  for (const raw of one) {
    const o = resolve(root, raw)
    if (o.const !== undefined) {
      const why = o.description ? ` — ${o.description}` : ''
      out.push({ value: String(o.const), label: `${String(o.const)}${why}` })
    } else if (Array.isArray(o.enum)) {
      out.push(...o.enum.map((v) => ({ value: String(v), label: String(v) })))
    } else {
      return []
    }
  }
  return out
}

function placeholder(def: unknown, kind: FieldKind): string {
  if (def === undefined || def === null) return ''
  if (kind === 'lines' && Array.isArray(def)) return def.join('\n')
  if (typeof def === 'string' || typeof def === 'number' || typeof def === 'boolean') return String(def)
  return JSON.stringify(def)
}

/**
 * The `config:` block from what was typed. Empty inputs are left out, so the
 * module's defaults apply. Returns the problems instead when there are any.
 */
export function buildConfig(list: Field[], values: Record<string, string>): { config: Record<string, unknown>; errors: Record<string, string> } {
  const config: Record<string, unknown> = {}
  const errors: Record<string, string> = {}
  for (const f of list) {
    const raw = (values[f.key] ?? '').trim()
    if (!raw) {
      if (f.required) errors[f.key] = 'Required.'
      continue
    }
    if (f.kind === 'integer' || f.kind === 'number') {
      const n = Number(raw)
      if (!Number.isFinite(n) || (f.kind === 'integer' && !Number.isInteger(n))) errors[f.key] = `Not ${f.kind === 'integer' ? 'a whole number' : 'a number'}.`
      else config[f.key] = n
    } else if (f.kind === 'boolean') {
      config[f.key] = raw === 'true'
    } else if (f.kind === 'lines') {
      config[f.key] = raw.split('\n').map((l) => l.trim()).filter(Boolean)
    } else if (f.kind === 'json') {
      try {
        config[f.key] = JSON.parse(raw)
      } catch {
        errors[f.key] = 'Not valid JSON.'
      }
    } else {
      config[f.key] = raw
    }
  }
  return { config, errors }
}

/** `id`, or `id-2`, `id-3`... the first one not in `taken`. */
export function freeId(id: string, taken: Set<string>): string {
  if (!taken.has(id)) return id
  let n = 2
  while (taken.has(`${id}-${n}`)) n++
  return `${id}-${n}`
}
